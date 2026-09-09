package main

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"embed"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unicode/utf16"
	"unsafe"

	"github.com/getlantern/systray"
	"github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
	"golang.org/x/net/http2"
	"golang.org/x/net/proxy"
	"golang.org/x/sys/windows"
	"gopkg.in/yaml.v3"
)

const maxRequestBytes = 64 << 20
const instanceMutexName = `Local\code-Manager-single-instance`
const managementListenAddress = "127.0.0.1:7780"
const runtimeConfigDirectoryName = "config"
const llmtrimDaemonAddress = "127.0.0.1:43117"
const upstreamAPIKeyPlaceholder = "PUT_YOUR_UPSTREAM_API_KEY_HERE"
const upstreamHandshakeTimeout = 10 * time.Second
const h3GracePeriod = 600 * time.Millisecond
const upstreamHandshakeAttempts = 5
const upstreamKeepAliveInterval = 20 * time.Second
const upstreamSessionLifetime = 10 * time.Minute
const upstreamSessionStreamLimit = 500
const llmtrimProxyIdleConnTimeout = 5 * time.Minute

const (
	proxyStateStopped     = "stopped"
	proxyStateConnecting  = "connecting"
	proxyStateRunning     = "running"
	proxyStateUnavailable = "unavailable"
)

var upstreamHandshakeRetryDelay = time.Second
var llmtrimRequestSequence uint64

const defaultConfigYAML = `# code-Manager configuration
listen_address: "127.0.0.1:7780"
upstream_base_url: "https://your-api.example.com"
upstream_api_key: "PUT_YOUR_UPSTREAM_API_KEY_HERE"
outbound_proxy: ""
llmtrim_path: ""
local_api_key: ""
upstream_websocket_enabled: true
startup_enabled: false
background_start: false
retry_enabled: false
retry_count: "5"
retry_interval_seconds: "1"
retry_status_codes: "100-199,300-399,401-407,409-499,500-503,505-523,525-599"
`

type Config struct {
	ListenAddress            string `yaml:"listen_address"`
	UpstreamBaseURL          string `yaml:"upstream_base_url"`
	UpstreamAPIKey           string `yaml:"upstream_api_key"`
	OutboundProxy            string `yaml:"outbound_proxy"`
	LLMTrimPath              string `yaml:"llmtrim_path"`
	LocalAPIKey              string `yaml:"local_api_key"`
	UpstreamWebSocketEnabled bool   `yaml:"upstream_websocket_enabled"`
	StartupEnabled           bool   `yaml:"startup_enabled"`
	BackgroundStart          bool   `yaml:"background_start"`
	RetryEnabled             bool   `yaml:"retry_enabled"`
	RetryCount               string `yaml:"retry_count"`
	RetryIntervalSeconds     string `yaml:"retry_interval_seconds"`
	RetryStatusCodes         string `yaml:"retry_status_codes"`
}

type gateway struct {
	configMu                    sync.RWMutex
	config                      Config
	configPath                  string
	client                      *http.Client
	llmtrimProxyClient          *http.Client
	llmtrimProxyMu              sync.Mutex
	llmtrimProxyFactory         func() (*http.Client, error)
	llmtrimRunningCheck         func(string) bool
	proxyHandler                http.Handler
	managementAddress           string
	logPath                     string
	logViewer                   logViewer
	llmtrimStatusViewer         llmtrimStatusViewer
	proxyMu                     sync.RWMutex
	proxyRunning                bool
	proxyState                  string
	proxyMessage                string
	proxyAddress                string
	proxyServer                 *http.Server
	proxyReady                  chan struct{}
	proxyReadyClosed            bool
	proxyReadyErr               error
	proxyStartCancel            context.CancelFunc
	proxyRunContext             context.Context
	proxyRunCancel              context.CancelFunc
	proxyGeneration             uint64
	shuttingDown                bool
	proxyRequests               sync.WaitGroup
	localHTTP1Requests          atomic.Int64
	webSocketMu                 sync.Mutex
	webSocketSessions           map[*webSocketSession]struct{}
	managementWebSocketMu       sync.Mutex
	managementWebSocketSessions map[*managementWebSocketSession]struct{}
	proxyConnectionMu           sync.Mutex
	proxyConnections            map[net.Conn]struct{}
	llmtrimMu                   sync.Mutex
	gortexMu                    sync.Mutex
	retryBodyCache              *retryBodyCache
	rtkMu                       sync.Mutex
	application                 *application
}

type proxyConnectionContextKey struct{}

type upstreamProtocol string

const (
	upstreamProtocolH2 upstreamProtocol = "h2"
	upstreamProtocolH3 upstreamProtocol = "h3"
)

type upstreamSession struct {
	protocol                upstreamProtocol
	roundTripper            http.RoundTripper
	available               func() bool
	ping                    func(context.Context) error
	createdAt               time.Time
	activeStreams           int
	activeWebSocketCarriers int
	closeOnce               sync.Once
	close                   func()
}

func (session *upstreamSession) Close() {
	if session == nil || session.close == nil {
		return
	}
	session.closeOnce.Do(session.close)
}

type upstreamDial struct {
	done    chan struct{}
	cancel  context.CancelFunc
	session *upstreamSession
	err     error
}

type upstreamHandshakeResult struct {
	protocol upstreamProtocol
	session  *upstreamSession
	err      error
}

// h3H2Transport resolves the upstream host once, then races HTTP/3 QUIC/TLS
// and HTTP/2 TCP/TLS against the same IP. It never sends a request until a
// winner is selected, so POST requests are not duplicated during the race.
type h3H2Transport struct {
	mu                sync.Mutex
	sessions          map[string][]*upstreamSession
	dialing           map[string]*upstreamDial
	closed            bool
	maintenanceCancel context.CancelFunc
	now               func() time.Time
	maxStreams        int
	sessionLifetime   time.Duration
	keepAliveInterval time.Duration

	lookupIP func(context.Context, string) (net.IP, error)
	dialH2   func(context.Context, string, string) (*upstreamSession, error)
	dialH3   func(context.Context, string, string) (*upstreamSession, error)
	grace    time.Duration
	fallback http.RoundTripper
}

var errH3H2TransportClosed = errors.New("H3/H2 transport is closed")

type settingsResponse struct {
	ListenAddress            string `json:"listen_address"`
	UpstreamBaseURL          string `json:"upstream_base_url"`
	UpstreamAPIKey           string `json:"upstream_api_key"`
	UpstreamWebSocketEnabled bool   `json:"upstream_websocket_enabled"`
	StartupEnabled           bool   `json:"startup_enabled"`
	BackgroundStart          bool   `json:"background_start"`
	RetryEnabled             bool   `json:"retry_enabled"`
	RetryCount               string `json:"retry_count"`
	RetryIntervalSeconds     string `json:"retry_interval_seconds"`
	RetryStatusCodes         string `json:"retry_status_codes"`
}

type settingUpdateRequest struct {
	Value string `json:"value"`
}

type settingUpdateResponse struct {
	Saved           bool   `json:"saved"`
	RequiresRestart bool   `json:"requires_restart"`
	Message         string `json:"message"`
	Value           string `json:"value"`
}

type llmtrimStatusResponse struct {
	Path            string `json:"path"`
	Version         string `json:"version"`
	Running         bool   `json:"running"`
	DesiredRunning  bool   `json:"desired_running"`
	ActivationState string `json:"activation_state"`
	Installed       bool   `json:"installed"`
	Configured      bool   `json:"configured"`
	DirectoryExists bool   `json:"directory_exists"`
	StateDirExists  bool   `json:"state_dir_exists"`
	TrayRunning     bool   `json:"tray_running"`
	Residual        bool   `json:"residual"`
	ProcessID       int    `json:"process_id,omitempty"`
	TrayProcessID   int    `json:"tray_process_id,omitempty"`
	Port            string `json:"port"`
	Message         string `json:"message"`
}

// llmtrimActivationSnapshot 将 daemon 的真实健康状态与受管状态账本分开。
// llmtrim 只有配置路径对应的进程和 43117 端口同时就绪时才算运行中。
type llmtrimActivationSnapshot struct {
	Installed               bool
	DirectoryExists         bool
	ConfiguredPath          string
	ConfiguredPathAvailable bool
	Running                 bool
	TrayRunning             bool
	ProcessID               int
	PortOpen                bool
	DesiredRunning          bool
	RecordedRunning         bool
	WindowsConfigured       bool
	StateReadFailed         bool
	StateNeedsAttention     bool
}

func llmtrimActivationState(snapshot llmtrimActivationSnapshot) string {
	if snapshot.StateReadFailed || snapshot.StateNeedsAttention {
		return "attention"
	}
	if snapshot.Running {
		if snapshot.WindowsConfigured {
			return "running"
		}
		return "attention"
	}
	if snapshot.TrayRunning || snapshot.ProcessID != 0 || snapshot.PortOpen || snapshot.DesiredRunning || snapshot.RecordedRunning {
		return "attention"
	}
	if snapshot.Installed {
		if strings.TrimSpace(snapshot.ConfiguredPath) == "" || !snapshot.ConfiguredPathAvailable {
			return "attention"
		}
		return "installed_stopped"
	}
	if snapshot.DirectoryExists || strings.TrimSpace(snapshot.ConfiguredPath) != "" {
		return "attention"
	}
	return "not_installed"
}

type llmtrimCommandRequest struct {
	Path string `json:"path"`
}

type proxyStatusResponse struct {
	Running       bool                     `json:"running"`
	State         string                   `json:"state"`
	ProcessID     int                      `json:"process_id"`
	ListenAddress string                   `json:"listen_address"`
	Message       string                   `json:"message"`
	Connections   connectionStatusResponse `json:"connections"`
}

type connectionStatusResponse struct {
	LocalHTTP1          int64 `json:"local_http1"`
	LocalWebSocket      int   `json:"local_ws"`
	UpstreamH2          int   `json:"upstream_h2"`
	UpstreamH2WebSocket bool  `json:"upstream_h2_ws"`
	UpstreamH3          int   `json:"upstream_h3"`
	UpstreamH3WebSocket bool  `json:"upstream_h3_ws"`
	UpstreamWebSocket   int   `json:"upstream_ws"`
	UpstreamStreams     int   `json:"upstream_streams"`
}

type upstreamConnectionStatus struct {
	h2Connections       int
	h2WebSocketCarriers int
	h3Connections       int
	h3WebSocketCarriers int
	webSocketCarriers   int
	activeStreams       int
}

type logStatusResponse struct {
	Showing bool   `json:"showing"`
	Message string `json:"message"`
}

type applicationIdentityResponse struct {
	ExecutablePath string `json:"executable_path"`
	Version        string `json:"version"`
}

type applicationExitResponse struct {
	Completed bool     `json:"completed"`
	Clean     bool     `json:"clean"`
	Message   string   `json:"message"`
	Warnings  []string `json:"warnings,omitempty"`
}

type applicationShutdownResult struct {
	Warnings []string
}

type logViewer struct {
	mu      sync.Mutex
	process windows.Handle
	pid     uint32
}

type llmtrimStatusViewer struct {
	mu      sync.Mutex
	process windows.Handle
	pid     uint32
}

var (
	llmtrimConsoleKernel32       = windows.NewLazySystemDLL("kernel32.dll")
	llmtrimAttachConsole         = llmtrimConsoleKernel32.NewProc("AttachConsole")
	llmtrimFreeConsole           = llmtrimConsoleKernel32.NewProc("FreeConsole")
	llmtrimSetConsoleCtrlHandler = llmtrimConsoleKernel32.NewProc("SetConsoleCtrlHandler")
)

type application struct {
	server                *http.Server
	listener              net.Listener
	localURL              string
	gateway               *gateway
	startupMode           bool
	updateExit            atomic.Bool
	shutdownOnce          sync.Once
	shutdownResourcesOnce sync.Once
	shutdownResult        applicationShutdownResult
}

// frontendFS 在构建时嵌入 Vue 的 dist 目录，首次发布只需分发一个 EXE。
//
//go:embed web/dist
var frontendFS embed.FS

//go:embed vision.md
var visionMetadata string

func applicationVersionFromVision(content string) (string, error) {
	key, version, found := strings.Cut(strings.TrimSpace(content), ":")
	if !found || strings.TrimSpace(key) != "vision" {
		return "", errors.New("vision.md 内容必须为 vision: <版本>")
	}
	version = strings.TrimSpace(version)
	if version == "" || strings.ContainsAny(version, "\r\n") {
		return "", errors.New("vision.md 中缺少有效版本号")
	}
	return version, nil
}

func embeddedApplicationVersion() (string, error) {
	return applicationVersionFromVision(visionMetadata)
}

//go:embed assets/tray.ico
var trayIcon []byte

//go:embed uninstall.bat.template
var uninstallBatTemplate []byte

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--cleanup-helper" {
		runCleanupHelper(os.Args[2:])
		return
	}
	if len(os.Args) == 2 && os.Args[1] == "--uninstall" {
		if err := runApplicationUninstall(); err != nil {
			fmt.Fprintln(os.Stderr, "卸载失败:", err)
			os.Exit(1)
		}
		return
	}
	if len(os.Args) > 1 && os.Args[1] == "gortex-bridge" {
		runGortexBridge(os.Args[2:])
		return
	}
	configPath := defaultConfigPath()
	startupMode := false
	for _, arg := range os.Args[1:] {
		switch arg {
		case "--startup":
			startupMode = true
		case "":
		default:
			log.Fatal("usage: code-Manager.exe [--startup|--uninstall]")
		}
	}
	if startupMode {
		// Windows Run 在用户登录后调用本程序；延迟初始化，避免桌面和网络尚未稳定时启动服务。
		time.Sleep(8 * time.Second)
	}
	instance, err := acquireSingleInstance(instanceMutexName)
	if err != nil {
		if errors.Is(err, errAlreadyRunning) {
			// 即使旧版进程仍在运行，也先让新 EXE 修复已有快捷方式的图标资源。
			// 之后仍遵守单实例语义：手动启动只打开现有页面，开机启动则静默退出。
			if executable, shortcutErr := currentCodeManagerExecutable(); shortcutErr != nil {
				log.Printf("刷新 code-Manager 快捷方式图标失败: %v", shortcutErr)
			} else if shortcutErr := refreshCodeManagerShortcutIcons(executable); shortcutErr != nil {
				log.Printf("刷新 code-Manager 快捷方式图标失败: %v", shortcutErr)
			}
			if !startupMode {
				openExistingInstance()
			}
			return
		}
		log.Fatal(err)
	}
	defer instance.Close()
	if err := launchExitCleanupHelper(); err != nil {
		log.Printf("启动退出清理助手失败: %v", err)
	}
	logFile, logPath, err := openSessionLog()
	if err != nil {
		log.Fatal(err)
	}
	defer logFile.Close()
	if err := ensureRuntimeUninstallScript(); err != nil {
		log.Fatalf("创建卸载脚本失败: %v", err)
	}
	created, err := ensureDefaultConfig(configPath)
	if err != nil {
		log.Fatal(err)
	}
	if created {
		log.Printf("config\\config.yaml 不存在，已创建默认配置：%s", configPath)
	}
	config, err := loadConfig(configPath)
	if err != nil {
		log.Fatal(err)
	}
	if err := initializeLLMTrimPath(configPath, &config); err != nil {
		log.Fatal(err)
	}
	repairInstalledGortexPath()
	if err := syncCodeManagerStartup(config.StartupEnabled); err != nil {
		log.Printf("同步 code-Manager 开机启动失败: %v", err)
	}
	client, err := makeHTTPClient(config.OutboundProxy)
	if err != nil {
		log.Fatal(err)
	}
	managementListener, err := net.Listen("tcp", managementListenAddress)
	if err != nil {
		log.Fatalf("listen management page on %s: %v", managementListenAddress, err)
	}
	managementAddress := managementListener.Addr().String()
	localURL := "http://" + managementAddress
	// 管理页面始终保持可用，代理转发必须由网页明确启动。
	retryCache := newRetryBodyCache(filepath.Dir(configPath))
	retryCache.cleanupStaleFiles()
	g := &gateway{config: config, configPath: configPath, client: client, managementAddress: managementAddress, logPath: logPath, proxyState: proxyStateStopped, retryBodyCache: retryCache}
	startGortexWatchEnforcer()
	mux := http.NewServeMux()
	mux.Handle("/", frontendHandler())
	mux.HandleFunc("/healthz", g.health)
	mux.HandleFunc("/api/events", g.managementEvents)
	mux.HandleFunc("/api/settings", g.settings)
	mux.HandleFunc("/api/settings/", g.updateSetting)
	mux.HandleFunc("/api/proxy", g.proxyStatus)
	mux.HandleFunc("/api/proxy/start", g.proxyStart)
	mux.HandleFunc("/api/proxy/stop", g.proxyStop)
	mux.HandleFunc("/api/logs", g.logStatus)
	mux.HandleFunc("/api/logs/show", g.showLogs)
	mux.HandleFunc("/api/logs/hide", g.hideLogs)
	mux.HandleFunc("/api/llmtrim/logs", g.llmtrimLogStatus)
	mux.HandleFunc("/api/llmtrim/logs/show", g.showLLMTrimLogs)
	mux.HandleFunc("/api/llmtrim/logs/hide", g.hideLLMTrimLogs)
	mux.HandleFunc("/api/llmtrim", g.llmtrimStatus)
	mux.HandleFunc("/api/llmtrim/start", g.llmtrimStart)
	mux.HandleFunc("/api/llmtrim/stop", g.llmtrimStop)
	mux.HandleFunc("/api/llmtrim/releases", g.llmtrimReleases)
	mux.HandleFunc("/api/llmtrim/install", g.llmtrimInstall)
	mux.HandleFunc("/api/llmtrim/uninstall", g.llmtrimUninstall)
	mux.HandleFunc("/api/rtk", g.rtkStatus)
	mux.HandleFunc("/api/rtk/releases", g.rtkReleases)
	mux.HandleFunc("/api/rtk/install", g.rtkInstall)
	mux.HandleFunc("/api/rtk/start", g.rtkStart)
	mux.HandleFunc("/api/rtk/stop", g.rtkStop)
	mux.HandleFunc("/api/rtk/uninstall", g.rtkUninstall)
	mux.HandleFunc("/api/snip", g.snipStatus)
	mux.HandleFunc("/api/snip/releases", g.snipReleases)
	mux.HandleFunc("/api/snip/install", g.snipInstall)
	mux.HandleFunc("/api/snip/start", g.snipStart)
	mux.HandleFunc("/api/snip/stop", g.snipStop)
	mux.HandleFunc("/api/snip/trust", g.snipTrust)
	mux.HandleFunc("/api/snip/uninstall", g.snipUninstall)
	mux.HandleFunc("/api/gortex", g.gortexStatus)
	mux.HandleFunc("/api/gortex/releases", g.gortexReleases)
	mux.HandleFunc("/api/gortex/install", g.gortexInstall)
	mux.HandleFunc("/api/gortex/start", g.gortexStart)
	mux.HandleFunc("/api/gortex/stop", g.gortexStop)
	mux.HandleFunc("/api/gortex/register", g.gortexRegister)
	mux.HandleFunc("/api/gortex/remove", g.gortexRemove)
	mux.HandleFunc("/api/gortex/trust", g.gortexTrust)
	mux.HandleFunc("/api/gortex/diagnostics", g.gortexDiagnostics)
	mux.HandleFunc("/api/gortex/track", g.gortexTrack)
	mux.HandleFunc("/api/gortex/untrack", g.gortexUntrack)
	mux.HandleFunc("/api/gortex/uninstall", g.gortexUninstall)
	mux.HandleFunc("/api/application/identity", g.applicationIdentity)
	mux.HandleFunc("/api/application/releases", g.applicationReleases)
	mux.HandleFunc("/api/application/update", g.applicationUpdate)
	mux.HandleFunc("/api/application/exit", g.applicationExit)
	mux.HandleFunc("/v1/", g.managementForward)
	proxyMux := http.NewServeMux()
	proxyMux.HandleFunc("/v1/", g.forward)
	g.proxyHandler = requestLogger(proxyMux)
	server := &http.Server{
		Addr:              managementListener.Addr().String(),
		Handler:           requestLogger(g.managementHandler(mux)),
		ReadHeaderTimeout: 10 * time.Second,
		ConnContext:       g.proxyConnectionContext,
		ConnState:         g.proxyConnectionState,
	}
	log.Printf("code-Manager management page listening at %s", localURL)
	log.Printf("proxy listen address from config: %s", config.ListenAddress)
	log.Printf("upstream: %s", redactURL(config.UpstreamBaseURL))
	if config.OutboundProxy == "" {
		log.Print("outbound proxy: direct connection")
	} else {
		log.Printf("outbound proxy: %s", redactURL(config.OutboundProxy))
	}
	log.Print("request bodies and API keys are never written to this log")
	app := &application{server: server, listener: managementListener, localURL: localURL, gateway: g, startupMode: startupMode}
	g.application = app
	systray.Run(app.onTrayReady, app.onTrayExit)
}

func (app *application) onTrayReady() {
	systray.SetIcon(trayIcon)
	go func() {
		executable, err := currentCodeManagerExecutable()
		if err != nil {
			log.Printf("刷新 code-Manager 快捷方式图标失败: %v", err)
			return
		}
		if err := refreshCodeManagerShortcutIcons(executable); err != nil {
			log.Printf("刷新 code-Manager 快捷方式图标失败: %v", err)
		}
	}()
	systray.SetTooltip("code-Manager 本地网关")
	systray.SetOnIconDoubleClick(func() {
		openBrowser(app.localURL)
	})
	systray.SetOnTaskbarActivate(func() {
		openBrowser(app.localURL)
	})
	systray.ShowTaskbarIcon()
	openPage := systray.AddMenuItem("打开网页", "在默认浏览器中打开 code-Manager")
	systray.AddSeparator()
	exitApp := systray.AddMenuItem("退出 code-Manager", "停止本地网关并退出")

	go func() {
		for {
			select {
			case <-openPage.ClickedCh:
				openBrowser(app.localURL)
			case <-exitApp.ClickedCh:
				app.shutdown()
				systray.Quit()
				return
			}
		}
	}()
	go app.restoreManagedTools()

	go func() {
		if err := app.server.Serve(app.listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Printf("local server stopped unexpectedly: %v", err)
		}
		app.shutdown()
		systray.Quit()
	}()

	if !app.startupMode {
		openBrowser(app.localURL)
		return
	}
	go app.runStartupSequence()
}

func (app *application) runStartupSequence() {
	config := app.gateway.currentConfig()
	if !config.StartupEnabled {
		log.Print("开机启动项已关闭，忽略 --startup 启动请求")
		return
	}
	if !config.BackgroundStart {
		openBrowser(app.localURL)
	}
	app.restoreManagedTools()
	llmDir, _ := llmtrimInstallDirectory()
	llmtrimDesired, _, _ := desiredManagedToolState(llmDir)
	llmtrimStarted, _, _, _ := queryLLMTrimRunning(app.gateway.currentConfig().LLMTrimPath)
	if !llmtrimDesired || !llmtrimStarted {
		log.Print("开机启动：llmtrim 未标记为恢复运行或未恢复成功，不启动代理")
		return
	}
	for attempt := 1; attempt <= 3; attempt++ {
		if err := app.gateway.startProxy(); err == nil {
			log.Printf("开机启动：代理已启动（第 %d 次尝试）", attempt)
			break
		} else {
			log.Printf("开机启动：代理第 %d 次尝试失败: %v", attempt, err)
		}
	}
}

func (app *application) restoreManagedTools() {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	rtkDir, _ := rtkInstallDirectory()
	snipDir, _ := snipInstallDirectory()
	rtkDesired, rtkStateExists, rtkErr := desiredManagedToolState(rtkDir)
	snipDesired, snipStateExists, snipErr := desiredManagedToolState(snipDir)
	if rtkErr != nil || snipErr != nil {
		log.Printf("恢复 RTK/snip 状态失败: rtk=%v snip=%v", rtkErr, snipErr)
		return
	}
	if rtkDesired && snipDesired {
		log.Print("检测到 RTK 与 snip 同时要求恢复，保持二者停止并标记冲突")
		return
	}
	if rtkStateExists && rtkDesired {
		if err := app.gateway.startRTK(ctx); err != nil {
			log.Printf("恢复 RTK 失败: %v", err)
		}
	}
	if snipStateExists && snipDesired {
		if err := app.gateway.startSnip(ctx); err != nil {
			log.Printf("恢复 snip 失败: %v", err)
		}
	}
	llmDir, _ := llmtrimInstallDirectory()
	if desired, exists, err := desiredManagedToolState(llmDir); err == nil && exists && desired {
		if err := app.gateway.startLLMTrim(ctx); err != nil {
			log.Printf("恢复 llmtrim 失败: %v", err)
		}
	}
}

func (app *application) onTrayExit() {
	app.shutdown()
}

func (app *application) shutdown() {
	if app.updateExit.Load() {
		app.shutdownForUpdate()
		return
	}
	app.shutdownOnce.Do(func() {
		app.shutdownManagedResources()
		if app.gateway != nil {
			app.gateway.closeManagementWebSocketSessions()
		}
		if app.server == nil {
			return
		}
		serverContext, serverCancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer serverCancel()
		if err := app.server.Shutdown(serverContext); err != nil {
			log.Printf("graceful shutdown failed: %v", err)
		}
	})
}

// beginUpdateExit stops only code-Manager itself. Managed tools stay alive so
// the replacement EXE can query their existing state after it starts.
func (app *application) beginUpdateExit() {
	app.updateExit.Store(true)
	app.shutdownForUpdate()
	systray.Quit()
}

func (app *application) shutdownForUpdate() {
	app.updateExit.Store(true)
	app.shutdownOnce.Do(func() {
		if app.gateway != nil {
			app.gateway.closeManagementWebSocketSessions()
		}
		if app.server == nil {
			return
		}
		serverContext, serverCancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer serverCancel()
		if err := app.server.Shutdown(serverContext); err != nil {
			log.Printf("update shutdown failed: %v", err)
		}
	})
}

func (app *application) shutdownManagedResources() applicationShutdownResult {
	app.shutdownResourcesOnce.Do(func() {
		result := applicationShutdownResult{}
		cleanupContext, cleanupCancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cleanupCancel()
		if app.gateway != nil {
			// 一旦接受退出请求，立即拒绝新的管理操作和代理请求。
			app.gateway.beginShutdown()
			appendShutdownWarning(&result, "关闭 llmtrim 状态窗口", app.gateway.llmtrimStatusViewer.stop())
			// 退出时和页面“停止代理”保持相同语义：先取消请求并关闭上游，
			// 再停止 llmtrim，避免压缩子进程仍被活动网关请求使用。
			appendShutdownWarning(&result, "停止代理", app.gateway.stopProxy(cleanupContext))
			// 退出顺序固定：先停止代理，再停止 llmtrim，最后关闭管理服务。
			appendShutdownWarning(&result, "停止 llmtrim", app.gateway.stopLLMTrim(cleanupContext))
			appendShutdownWarning(&result, "停止 RTK", app.gateway.stopRTK(cleanupContext))
			appendShutdownWarning(&result, "停止 snip", app.gateway.stopSnip(cleanupContext))
			appendShutdownWarning(&result, "停止 Gortex", app.gateway.stopGortex(cleanupContext))
			app.gateway.closeHTTPClient()
			appendShutdownWarning(&result, "关闭 code-Manager 日志窗口", app.gateway.logViewer.stop())
		}
		app.shutdownResult = result
	})
	return app.shutdownResult
}

func appendShutdownWarning(result *applicationShutdownResult, operation string, err error) {
	if err == nil {
		return
	}
	log.Printf("shutdown: %s failed: %v", operation, err)
	result.Warnings = append(result.Warnings, operation+"："+err.Error())
}

func defaultConfigPath() string {
	directory, err := runtimeConfigDirectory()
	if err != nil {
		return filepath.Join(runtimeConfigDirectoryName, "config.yaml")
	}
	return filepath.Join(directory, "config.yaml")
}

func runtimeConfigDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("定位 code-Manager.exe 失败: %w", err)
	}
	return filepath.Join(filepath.Dir(executable), runtimeConfigDirectoryName), nil
}

func ensureDefaultConfig(configPath string) (bool, error) {
	if err := os.MkdirAll(filepath.Dir(configPath), 0700); err != nil {
		return false, fmt.Errorf("创建配置目录失败: %w", err)
	}
	info, err := os.Stat(configPath)
	if err == nil {
		if info.IsDir() {
			return false, fmt.Errorf("config path %q is a directory", configPath)
		}
		return false, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return false, fmt.Errorf("stat config %q: %w", configPath, err)
	}

	file, err := os.OpenFile(configPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		if errors.Is(err, os.ErrExist) {
			return false, nil
		}
		return false, fmt.Errorf("create default config %q: %w", configPath, err)
	}
	if _, err := io.WriteString(file, defaultConfigYAML); err != nil {
		_ = file.Close()
		_ = os.Remove(configPath)
		return false, fmt.Errorf("write default config %q: %w", configPath, err)
	}
	if err := file.Close(); err != nil {
		_ = os.Remove(configPath)
		return false, fmt.Errorf("close default config %q: %w", configPath, err)
	}
	return true, nil
}

func openSessionLog() (*os.File, string, error) {
	directory, err := runtimeConfigDirectory()
	if err != nil {
		return nil, "", err
	}
	if err := os.MkdirAll(directory, 0700); err != nil {
		return nil, "", fmt.Errorf("创建配置目录失败: %w", err)
	}
	logPath := filepath.Join(directory, "code-Manager.log")
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0644)
	if err != nil {
		return nil, "", fmt.Errorf("open log file %q: %w", logPath, err)
	}
	log.SetOutput(logFile)
	return logFile, logPath, nil
}

func (viewer *logViewer) start(logPath string) error {
	viewer.mu.Lock()
	defer viewer.mu.Unlock()
	if viewer.process != 0 {
		return nil
	}

	commandInterpreter, err := exec.LookPath("cmd.exe")
	if err != nil {
		return fmt.Errorf("locate cmd.exe: %w", err)
	}
	applicationName, err := windows.UTF16PtrFromString(commandInterpreter)
	if err != nil {
		return fmt.Errorf("encode cmd.exe path: %w", err)
	}
	workingDirectory, err := windows.UTF16PtrFromString(filepath.Dir(logPath))
	if err != nil {
		return fmt.Errorf("encode log directory: %w", err)
	}
	// GUI EXE 通过 os/exec 启动的 CMD 会继承 NUL 标准流。必须由 CreateProcess 创建独立控制台，
	// 不能只将 PowerShell 输出重定向到 CONOUT$，否则 PowerShell 仍会处于重定向输出模式。
	commandLine, err := windows.UTF16FromString(logViewerCommandLine(commandInterpreter, logPath))
	if err != nil {
		return fmt.Errorf("encode log viewer CMD command: %w", err)
	}
	startupInfo := windows.StartupInfo{Cb: uint32(unsafe.Sizeof(windows.StartupInfo{}))}
	var processInfo windows.ProcessInformation
	if err := windows.CreateProcess(
		applicationName,
		&commandLine[0],
		nil,
		nil,
		false,
		windows.CREATE_NEW_CONSOLE,
		nil,
		workingDirectory,
		&startupInfo,
		&processInfo,
	); err != nil {
		return fmt.Errorf("start log viewer CMD window: %w", err)
	}
	if err := windows.CloseHandle(processInfo.Thread); err != nil {
		_ = windows.CloseHandle(processInfo.Process)
		return fmt.Errorf("close log viewer CMD thread handle: %w", err)
	}

	viewer.process = processInfo.Process
	viewer.pid = processInfo.ProcessId
	go viewer.wait(processInfo.Process)
	return nil
}

func logViewerCommandLine(commandInterpreter, logPath string) string {
	// EncodedCommand keeps spaces, backslashes, and quotes in logPath out of cmd.exe parsing.
	viewerScript := fmt.Sprintf(
		`$Host.UI.RawUI.WindowTitle = 'code-Manager 日志'; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8; Get-Content -LiteralPath %s -Encoding UTF8 -Wait`,
		quotePowerShellString(logPath),
	)
	return windows.EscapeArg(commandInterpreter) + ` /d /k "chcp 65001>nul & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand ` + encodePowerShellCommand(viewerScript) + `"`
}

func quotePowerShellString(value string) string {
	return "'" + strings.ReplaceAll(value, "'", "''") + "'"
}

func encodePowerShellCommand(script string) string {
	codeUnits := utf16.Encode([]rune(script))
	bytesValue := make([]byte, len(codeUnits)*2)
	for index, codeUnit := range codeUnits {
		bytesValue[index*2] = byte(codeUnit)
		bytesValue[index*2+1] = byte(codeUnit >> 8)
	}
	return base64.StdEncoding.EncodeToString(bytesValue)
}

func (viewer *logViewer) stop() error {
	viewer.mu.Lock()
	process := viewer.process
	pid := viewer.pid
	viewer.mu.Unlock()
	if process == 0 || pid == 0 {
		return nil
	}
	// 结束日志窗口及其 PowerShell 子进程，不触碰 code-Manager 或 llmtrim。
	killTree := exec.Command("taskkill.exe", "/PID", strconv.Itoa(int(pid)), "/T", "/F")
	killTree.Stdout = io.Discard
	killTree.Stderr = io.Discard
	if err := killTree.Run(); err != nil {
		state, waitErr := windows.WaitForSingleObject(process, 0)
		if waitErr != nil || state != windows.WAIT_OBJECT_0 {
			return fmt.Errorf("close log viewer CMD window: %w", err)
		}
	}
	viewer.mu.Lock()
	if viewer.process == process {
		viewer.process = 0
		viewer.pid = 0
	}
	viewer.mu.Unlock()
	return nil
}

func (viewer *logViewer) running() bool {
	viewer.mu.Lock()
	defer viewer.mu.Unlock()
	return viewer.process != 0
}

func (viewer *logViewer) wait(process windows.Handle) {
	_, _ = windows.WaitForSingleObject(process, windows.INFINITE)
	_ = windows.CloseHandle(process)
	viewer.mu.Lock()
	if viewer.process == process {
		viewer.process = 0
		viewer.pid = 0
	}
	viewer.mu.Unlock()
}

// llmtrim status 的工作目录和关闭方式不同：它必须在 llmtrim.exe 所在目录运行，并先接收 Ctrl+C。
func (viewer *llmtrimStatusViewer) start(executable string) error {
	executable, err := validateLLMTrimPath(executable)
	if err != nil {
		return err
	}

	viewer.mu.Lock()
	defer viewer.mu.Unlock()
	if viewer.process != 0 {
		return nil
	}

	commandInterpreter, err := exec.LookPath("cmd.exe")
	if err != nil {
		return fmt.Errorf("locate cmd.exe: %w", err)
	}
	applicationName, err := windows.UTF16PtrFromString(commandInterpreter)
	if err != nil {
		return fmt.Errorf("encode cmd.exe path: %w", err)
	}
	workingDirectory, err := windows.UTF16PtrFromString(filepath.Dir(executable))
	if err != nil {
		return fmt.Errorf("encode llmtrim directory: %w", err)
	}
	// CreateProcess 的当前目录等价于先 cd 到 llmtrim 目录，status 始终使用相对路径启动。
	commandLine, err := windows.UTF16FromString(
		windows.EscapeArg(commandInterpreter) + ` /d /k "chcp 65001>nul & title llmtrim status & .\llmtrim.exe status"`,
	)
	if err != nil {
		return fmt.Errorf("encode llmtrim CMD command: %w", err)
	}
	startupInfo := windows.StartupInfo{Cb: uint32(unsafe.Sizeof(windows.StartupInfo{}))}
	var processInfo windows.ProcessInformation
	if err := windows.CreateProcess(
		applicationName,
		&commandLine[0],
		nil,
		nil,
		false,
		windows.CREATE_NEW_CONSOLE,
		nil,
		workingDirectory,
		&startupInfo,
		&processInfo,
	); err != nil {
		return fmt.Errorf("start llmtrim CMD window: %w", err)
	}
	if err := windows.CloseHandle(processInfo.Thread); err != nil {
		_ = windows.CloseHandle(processInfo.Process)
		return fmt.Errorf("close llmtrim CMD thread handle: %w", err)
	}

	viewer.process = processInfo.Process
	viewer.pid = processInfo.ProcessId
	go viewer.wait(processInfo.Process)
	return nil
}

func (viewer *llmtrimStatusViewer) wait(process windows.Handle) {
	_, _ = windows.WaitForSingleObject(process, windows.INFINITE)
	_ = windows.CloseHandle(process)
	viewer.mu.Lock()
	if viewer.process == process {
		viewer.process = 0
		viewer.pid = 0
	}
	viewer.mu.Unlock()
}

func (viewer *llmtrimStatusViewer) stop() error {
	viewer.mu.Lock()
	process := viewer.process
	pid := viewer.pid
	viewer.mu.Unlock()
	if process == 0 || pid == 0 {
		return nil
	}

	// 先向独立控制台发送 Ctrl+C，让 llmtrim.exe status 有机会自行退出；随后关闭 CMD 及其子进程树。
	if err := sendLLMTrimConsoleCtrlC(pid); err != nil {
		log.Printf("send Ctrl+C to llmtrim status window failed, will force close: %v", err)
	}
	time.Sleep(300 * time.Millisecond)
	killTree := exec.Command("taskkill.exe", "/PID", strconv.Itoa(int(pid)), "/T", "/F")
	killTree.Stdout = io.Discard
	killTree.Stderr = io.Discard
	if err := killTree.Run(); err != nil {
		state, waitErr := windows.WaitForSingleObject(process, 0)
		if waitErr != nil || state != windows.WAIT_OBJECT_0 {
			return fmt.Errorf("close llmtrim CMD window: %w", err)
		}
	}
	viewer.mu.Lock()
	if viewer.process == process {
		viewer.process = 0
		viewer.pid = 0
	}
	viewer.mu.Unlock()
	return nil
}

func (viewer *llmtrimStatusViewer) running() bool {
	viewer.mu.Lock()
	defer viewer.mu.Unlock()
	return viewer.process != 0
}

func sendLLMTrimConsoleCtrlC(processID uint32) error {
	// 正式 EXE 是 GUI 子系统，通常没有控制台；先主动脱离可能存在的调试控制台，确保连接到目标 CMD。
	_, _, _ = llmtrimFreeConsole.Call()
	if result, _, err := llmtrimAttachConsole.Call(uintptr(processID)); result == 0 {
		return fmt.Errorf("attach llmtrim CMD console: %w", err)
	}
	defer llmtrimFreeConsole.Call()
	if result, _, err := llmtrimSetConsoleCtrlHandler.Call(0, 1); result == 0 {
		return fmt.Errorf("ignore local Ctrl+C while stopping llmtrim: %w", err)
	}
	defer llmtrimSetConsoleCtrlHandler.Call(0, 0)
	if err := windows.GenerateConsoleCtrlEvent(windows.CTRL_C_EVENT, 0); err != nil {
		return fmt.Errorf("send Ctrl+C to llmtrim CMD console: %w", err)
	}
	return nil
}

func validateListenAddress(address string) error {
	if address == "" {
		return errors.New("listen_address is required")
	}
	host, rawPort, err := net.SplitHostPort(address)
	if err != nil {
		return errors.New("listen_address must use IPv4:port or [IPv6]:port format")
	}
	ip := net.ParseIP(host)
	if ip == nil {
		return errors.New("listen_address host must be a single IPv4 or IPv6 address")
	}
	if ip.To4() != nil && strings.HasPrefix(address, "[") {
		return errors.New("IPv4 listen_address must use IPv4:port format")
	}
	if ip.To4() == nil && !strings.HasPrefix(address, "[") {
		return errors.New("IPv6 listen_address must use [IPv6]:port format")
	}
	port, err := strconv.Atoi(rawPort)
	if err != nil || port < 1 || port > 65535 {
		return errors.New("listen_address port must be between 1 and 65535")
	}
	return nil
}

func sameListenAddress(left, right string) bool {
	if left == "" || right == "" {
		return false
	}
	leftTCP, leftErr := net.ResolveTCPAddr("tcp", left)
	rightTCP, rightErr := net.ResolveTCPAddr("tcp", right)
	if leftErr != nil || rightErr != nil || leftTCP.Port != rightTCP.Port {
		return false
	}
	return leftTCP.IP.Equal(rightTCP.IP)
}

func validateUpstreamBaseURL(rawURL string) error {
	if strings.TrimSpace(rawURL) != rawURL || rawURL == "" {
		return errors.New("upstream_base_url must be a valid https URL")
	}
	upstream, err := url.ParseRequestURI(rawURL)
	if err != nil || upstream.Scheme != "https" || upstream.Host == "" || upstream.User != nil || upstream.Fragment != "" {
		return errors.New("upstream_base_url must be a valid https URL")
	}
	host := upstream.Hostname()
	if host == "" {
		return errors.New("upstream_base_url must include a host")
	}
	if strings.HasSuffix(upstream.Host, ":") {
		return errors.New("upstream_base_url port must be between 1 and 65535")
	}
	if port := upstream.Port(); port != "" {
		value, err := strconv.Atoi(port)
		if err != nil || value < 1 || value > 65535 {
			return errors.New("upstream_base_url port must be between 1 and 65535")
		}
	}
	if strings.Contains(host, ":") && (net.ParseIP(host) == nil || !strings.HasPrefix(upstream.Host, "[")) {
		return errors.New("IPv6 upstream_base_url must use [IPv6] notation")
	}
	return nil
}

func validateUpstreamAPIKey(value string) error {
	if strings.TrimSpace(value) == "" || strings.TrimSpace(value) == upstreamAPIKeyPlaceholder {
		return errors.New("请先设置有效的 upstream_api_key")
	}
	return nil
}

func validateProxyConfiguration(config Config) error {
	if err := validateUpstreamBaseURL(config.UpstreamBaseURL); err != nil {
		return err
	}
	return validateUpstreamAPIKey(config.UpstreamAPIKey)
}

func isCodeManagerRunning(localURL string) bool {
	client := &http.Client{Timeout: time.Second}
	response, err := client.Get(localURL + "/healthz")
	if err != nil {
		return false
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 128))
	return err == nil && response.StatusCode == http.StatusOK && bytes.Equal(bytes.TrimSpace(body), []byte(`{"status":"ok"}`))
}

func openExistingInstance() {
	localURL := "http://" + managementListenAddress
	if waitForService(localURL, 5*time.Second) {
		openBrowser(localURL)
		return
	}
	// 即使服务正处于启动阶段，也交给默认浏览器处理，确保用户点击后总能得到页面。
	openBrowser(localURL)
}

func waitForService(localURL string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if isCodeManagerRunning(localURL) {
			return true
		}
		time.Sleep(100 * time.Millisecond)
	}
	return false
}

func openBrowser(target string) {
	if err := exec.Command("rundll32.exe", "url.dll,FileProtocolHandler", target).Start(); err != nil {
		log.Printf("open browser failed: %v; open %s manually", err, target)
	}
}

func frontendHandler() http.Handler {
	root, err := fs.Sub(frontendFS, "web/dist")
	if err != nil {
		log.Fatalf("load embedded frontend: %v", err)
	}
	files := http.FileServer(http.FS(root))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		requestPath := strings.TrimPrefix(path.Clean(r.URL.Path), "/")
		if requestPath == "" {
			requestPath = "index.html"
		}
		if _, err := fs.Stat(root, requestPath); err != nil {
			// Vue Router 的 history 路由回退到入口页面。
			r.URL.Path = "/index.html"
		}
		files.ServeHTTP(w, r)
	})
}

func (g *gateway) managementHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if g.isShuttingDown() && r.URL.Path != "/api/application/exit" {
			http.Error(w, "code-Manager 正在退出，管理操作已停止", http.StatusServiceUnavailable)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func loadConfig(configPath string) (Config, error) {
	data, err := os.ReadFile(configPath)
	if err != nil {
		return Config{}, fmt.Errorf("read config %q: %w", configPath, err)
	}
	var config Config
	if err := yaml.Unmarshal(data, &config); err != nil {
		return Config{}, fmt.Errorf("parse config %q: %w", configPath, err)
	}
	if err := applyRetryConfigDefaults(data, &config); err != nil {
		return Config{}, fmt.Errorf("parse retry config %q: %w", configPath, err)
	}
	config.RetryCount = normalizeRetryCount(config.RetryCount)
	config.RetryIntervalSeconds = normalizeRetryInterval(config.RetryIntervalSeconds)
	config.RetryStatusCodes, _ = normalizeRetryStatusCodes(config.RetryStatusCodes)
	if err := validateListenAddress(config.ListenAddress); err != nil {
		return Config{}, err
	}
	if err := validateUpstreamBaseURL(config.UpstreamBaseURL); err != nil {
		return Config{}, err
	}
	if config.LLMTrimPath != "" {
		if _, err := os.Stat(config.LLMTrimPath); err != nil {
			return Config{}, fmt.Errorf("llmtrim_path is unavailable: %w", err)
		}
	}
	if config.OutboundProxy != "" {
		proxyURL, err := url.Parse(config.OutboundProxy)
		if err != nil || proxyURL.Host == "" || (proxyURL.Scheme != "http" && proxyURL.Scheme != "socks5") {
			return Config{}, errors.New("outbound_proxy must be empty, http://host:port, or socks5://host:port")
		}
	}
	return config, nil
}

func makeHTTPClient(rawProxy string) (*http.Client, error) {
	if rawProxy == "" {
		return &http.Client{Transport: newH3H2Transport(), CheckRedirect: keepUpstreamRedirectResponse}, nil
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.ForceAttemptHTTP2 = true
	if rawProxy != "" {
		proxyURL, _ := url.Parse(rawProxy)
		if proxyURL.Scheme == "socks5" {
			dialer, err := proxy.SOCKS5("tcp", proxyURL.Host, nil, proxy.Direct)
			if err != nil {
				return nil, err
			}
			transport.Proxy = nil
			transport.Dial = dialer.Dial
		} else {
			transport.Proxy = http.ProxyURL(proxyURL)
		}
	}
	return &http.Client{Transport: newWebSocketHTTPTransport(transport, rawProxy), CheckRedirect: keepUpstreamRedirectResponse}, nil
}

// newLLMTrimProxyClient creates the official llmtrim topology: the request
// keeps its real upstream URL while the HTTP transport uses 43117 as a proxy.
func newLLMTrimProxyClient() (*http.Client, error) {
	caPath, err := llmtrimCAPEMPath()
	if err != nil {
		return nil, err
	}
	return newLLMTrimProxyClientWithCA(llmtrimProxyURL, caPath)
}

func newLLMTrimProxyClientWithCA(rawProxyURL, caPath string) (*http.Client, error) {
	proxyURL, err := url.Parse(rawProxyURL)
	if err != nil || proxyURL.Scheme != "http" || proxyURL.Host == "" {
		return nil, errors.New("invalid llmtrim proxy URL")
	}
	caPEM, err := os.ReadFile(caPath)
	if err != nil {
		return nil, fmt.Errorf("read llmtrim CA %q: %w", caPath, err)
	}
	// This client is exclusively for the llmtrim MITM route. Trusting the
	// system pool here would silently accept a transparent upstream tunnel if
	// llmtrim did not intercept the configured host, so only trust its CA.
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM(caPEM) {
		return nil, fmt.Errorf("parse llmtrim CA %q", caPath)
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = http.ProxyURL(proxyURL)
	transport.TLSClientConfig = &tls.Config{RootCAs: roots, MinVersion: tls.VersionTLS12}
	transport.ForceAttemptHTTP2 = true
	transport.MaxIdleConns = 64
	transport.MaxIdleConnsPerHost = 16
	// Keep the explicit proxy tunnel reusable for five minutes. Do not set a
	// client-wide request deadline here: OpenAI-compatible streaming responses
	// must be allowed to outlive an idle connection timeout.
	transport.IdleConnTimeout = llmtrimProxyIdleConnTimeout
	return &http.Client{Transport: transport, CheckRedirect: keepUpstreamRedirectResponse}, nil
}

func llmtrimCAPEMPath() (string, error) {
	userProfile := strings.TrimSpace(os.Getenv("USERPROFILE"))
	if userProfile == "" {
		return "", errors.New("USERPROFILE is not set; cannot locate llmtrim CA")
	}
	return filepath.Join(userProfile, ".llmtrim", "ca.pem"), nil
}

func keepUpstreamRedirectResponse(_ *http.Request, _ []*http.Request) error {
	return http.ErrUseLastResponse
}

func newH3H2Transport() *h3H2Transport {
	fallback := http.DefaultTransport.(*http.Transport).Clone()
	fallback.ForceAttemptHTTP2 = true
	transport := &h3H2Transport{
		sessions:          make(map[string][]*upstreamSession),
		dialing:           make(map[string]*upstreamDial),
		grace:             h3GracePeriod,
		fallback:          fallback,
		now:               time.Now,
		maxStreams:        upstreamSessionStreamLimit,
		sessionLifetime:   upstreamSessionLifetime,
		keepAliveInterval: upstreamKeepAliveInterval,
	}
	transport.lookupIP = lookupUpstreamIP
	transport.dialH2 = dialUpstreamH2
	transport.dialH3 = dialUpstreamH3
	return transport
}

func lookupUpstreamIP(ctx context.Context, host string) (net.IP, error) {
	if ip := net.ParseIP(host); ip != nil {
		return ip, nil
	}
	addresses, err := net.DefaultResolver.LookupIP(ctx, "ip", host)
	if err != nil {
		return nil, err
	}
	if len(addresses) == 0 {
		return nil, fmt.Errorf("no IP address found for %s", host)
	}
	return addresses[0], nil
}

func dialUpstreamH2(ctx context.Context, host, address string) (*upstreamSession, error) {
	dialer := &net.Dialer{}
	connection, err := dialer.DialContext(ctx, "tcp", address)
	if err != nil {
		return nil, err
	}
	// Go 对字面 IP 不发送 SNI，但仍会用 ServerName 完成 IP SAN 证书校验。
	tlsConfig := &tls.Config{ServerName: host, NextProtos: []string{"h2"}, MinVersion: tls.VersionTLS12}
	tlsConnection := tls.Client(connection, tlsConfig)
	if err := tlsConnection.HandshakeContext(ctx); err != nil {
		_ = connection.Close()
		return nil, err
	}
	if tlsConnection.ConnectionState().NegotiatedProtocol != "h2" {
		_ = tlsConnection.Close()
		return nil, errors.New("upstream did not negotiate HTTP/2")
	}
	h2Transport := &http2.Transport{TLSClientConfig: tlsConfig.Clone()}
	clientConnection, err := h2Transport.NewClientConn(tlsConnection)
	if err != nil {
		_ = tlsConnection.Close()
		return nil, err
	}
	if err := clientConnection.Ping(ctx); err != nil {
		_ = clientConnection.Close()
		_ = tlsConnection.Close()
		return nil, fmt.Errorf("upstream HTTP/2 ping failed: %w", err)
	}
	return &upstreamSession{
		protocol:     upstreamProtocolH2,
		roundTripper: clientConnection,
		available: func() bool {
			state := clientConnection.State()
			return !state.Closed && !state.Closing
		},
		ping:      clientConnection.Ping,
		createdAt: time.Now(),
		close: func() {
			_ = clientConnection.Close()
			_ = tlsConnection.Close()
		},
	}, nil
}

func dialUpstreamH3(ctx context.Context, host, address string) (*upstreamSession, error) {
	udpAddress, err := net.ResolveUDPAddr("udp", address)
	if err != nil {
		return nil, err
	}
	network := "udp6"
	if udpAddress.IP.To4() != nil {
		network = "udp4"
	}
	udpConnection, err := net.ListenUDP(network, nil)
	if err != nil {
		return nil, err
	}
	// Go 对字面 IP 不发送 SNI，但仍会用 ServerName 完成 IP SAN 证书校验。
	tlsConfig := &tls.Config{ServerName: host, NextProtos: []string{http3.NextProtoH3}, MinVersion: tls.VersionTLS13}
	quicConnection, err := quic.Dial(ctx, udpConnection, udpAddress, tlsConfig, &quic.Config{
		HandshakeIdleTimeout: upstreamHandshakeTimeout,
		KeepAlivePeriod:      upstreamKeepAliveInterval,
		MaxIdleTimeout:       upstreamSessionLifetime,
	})
	if err != nil {
		_ = udpConnection.Close()
		return nil, err
	}
	h3Transport := &http3.Transport{}
	clientConnection := h3Transport.NewClientConn(quicConnection)
	select {
	case <-clientConnection.ReceivedSettings():
	case <-ctx.Done():
		_ = clientConnection.CloseWithError(0, "context canceled")
		_ = quicConnection.CloseWithError(0, "context canceled")
		_ = udpConnection.Close()
		return nil, context.Cause(ctx)
	}
	return &upstreamSession{
		protocol:     upstreamProtocolH3,
		roundTripper: clientConnection,
		available: func() bool {
			select {
			case <-clientConnection.Context().Done():
				return false
			default:
				return true
			}
		},
		ping: func(context.Context) error {
			select {
			case <-clientConnection.Context().Done():
				return context.Cause(clientConnection.Context())
			default:
				return nil
			}
		},
		createdAt: time.Now(),
		close: func() {
			_ = clientConnection.CloseWithError(0, "transport closed")
			_ = quicConnection.CloseWithError(0, "transport closed")
			_ = udpConnection.Close()
		},
	}, nil
}

func (t *h3H2Transport) RoundTrip(request *http.Request) (*http.Response, error) {
	if shouldTryHTTPOverWebSocket(request) {
		if response, attempted, err := t.tryRoundTripHTTPOverWebSocket(request); attempted {
			return response, err
		}
	}
	return t.roundTripHTTP(request)
}

func (t *h3H2Transport) roundTripHTTP(request *http.Request) (*http.Response, error) {
	if request.URL == nil || request.URL.Scheme != "https" {
		return t.fallback.RoundTrip(request)
	}
	session, release, err := t.sessionFor(request.Context(), request.URL.Host)
	if err != nil {
		return nil, err
	}
	response, err := session.roundTripper.RoundTrip(request)
	if err != nil {
		release()
		// A canceled or reset request stream must not tear down the shared
		// physical connection. Only discard it when the transport reports that
		// it can no longer accept requests.
		if !sessionAvailable(session) {
			t.removeSession(request.URL.Host, session)
		}
		return nil, err
	}
	if response.Body == nil {
		release()
		return response, nil
	}
	response.Body = &releaseOnClose{ReadCloser: response.Body, release: release}
	return response, nil
}

type releaseOnClose struct {
	io.ReadCloser
	once    sync.Once
	release func()
}

func (body *releaseOnClose) Read(buffer []byte) (int, error) {
	count, err := body.ReadCloser.Read(buffer)
	if err != nil {
		body.finish()
	}
	return count, err
}

func (body *releaseOnClose) Close() error {
	err := body.ReadCloser.Close()
	body.finish()
	return err
}

func (body *releaseOnClose) finish() {
	body.once.Do(body.release)
}

func (t *h3H2Transport) Prewarm(ctx context.Context, rawURL string) error {
	upstream, err := url.Parse(rawURL)
	if err != nil || upstream.Scheme != "https" || upstream.Host == "" {
		return errors.New("upstream_base_url must be a valid https URL")
	}
	_, release, err := t.sessionFor(ctx, upstream.Host)
	if err != nil {
		return err
	}
	release()
	return nil
}

func (t *h3H2Transport) StartMaintenance(ctx context.Context) {
	t.mu.Lock()
	if t.closed || t.maintenanceCancel != nil {
		t.mu.Unlock()
		return
	}
	maintenanceContext, cancel := context.WithCancel(ctx)
	t.maintenanceCancel = cancel
	t.mu.Unlock()
	go t.maintain(maintenanceContext)
}

func (t *h3H2Transport) maintain(ctx context.Context) {
	interval := t.keepAliveInterval
	if interval <= 0 {
		interval = upstreamKeepAliveInterval
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			t.maintainSessions(ctx)
		}
	}
}

func (t *h3H2Transport) maintainSessions(ctx context.Context) {
	t.mu.Lock()
	if t.closed {
		t.mu.Unlock()
		return
	}
	now := t.currentTime()
	checks := make([]struct {
		hostPort string
		session  *upstreamSession
	}, 0)
	toClose := make([]*upstreamSession, 0)
	for hostPort, sessions := range t.sessions {
		kept := sessions[:0]
		removed := false
		needsReplacement := false
		for _, session := range sessions {
			if !sessionAvailable(session) {
				if session != nil {
					toClose = append(toClose, session)
				}
				removed = true
				continue
			}
			expired := t.sessionExpired(session, now)
			if expired && session.activeStreams == 0 {
				toClose = append(toClose, session)
				removed = true
				continue
			}
			kept = append(kept, session)
			if expired {
				// Keep active streams alive while establishing a replacement before
				// the next request needs capacity.
				needsReplacement = true
			}
			checks = append(checks, struct {
				hostPort string
				session  *upstreamSession
			}{hostPort: hostPort, session: session})
		}
		if len(kept) == 0 {
			delete(t.sessions, hostPort)
			t.startDialLocked(hostPort)
			continue
		}
		t.sessions[hostPort] = kept
		if removed || needsReplacement {
			t.startDialLocked(hostPort)
		}
	}
	t.mu.Unlock()
	for _, session := range toClose {
		session.Close()
	}
	for _, check := range checks {
		if check.session.ping == nil {
			continue
		}
		pingContext, cancel := context.WithTimeout(ctx, upstreamHandshakeTimeout)
		err := check.session.ping(pingContext)
		cancel()
		if err != nil {
			log.Printf("upstream %s keep-alive failed: %v", check.hostPort, err)
			t.mu.Lock()
			removed := t.removeSessionLocked(check.hostPort, check.session)
			if removed && !t.closed {
				// Rebuild failed physical connections even when other sessions for
				// this upstream remain in the pool.
				t.startDialLocked(check.hostPort)
			}
			t.mu.Unlock()
			if removed {
				check.session.Close()
			}
		}
	}
}

func (t *h3H2Transport) sessionFor(ctx context.Context, hostPort string) (*upstreamSession, func(), error) {
	for {
		t.mu.Lock()
		if t.closed {
			t.mu.Unlock()
			return nil, nil, errH3H2TransportClosed
		}
		if session := t.acquireSessionLocked(hostPort, false); session != nil {
			t.mu.Unlock()
			return session, t.releaseSession(hostPort, session), nil
		}
		dialing := t.startDialLocked(hostPort)
		// Once a pool already has usable sessions, capacity expansion must not
		// hold up a new request. Start the extra connection in the background
		// and temporarily reuse the least busy session over its normal limit.
		if session := t.acquireSessionLocked(hostPort, true); session != nil {
			t.mu.Unlock()
			return session, t.releaseSession(hostPort, session), nil
		}
		t.mu.Unlock()

		select {
		case <-dialing.done:
			if dialing.err == nil && dialing.session != nil {
				continue
			}
			t.mu.Lock()
			fallback := t.acquireSessionLocked(hostPort, true)
			t.mu.Unlock()
			if fallback != nil {
				return fallback, t.releaseSession(hostPort, fallback), nil
			}
			if dialing.err != nil {
				return nil, nil, dialing.err
			}
			return nil, nil, errors.New("upstream handshake did not produce a usable session")
		case <-ctx.Done():
			return nil, nil, context.Cause(ctx)
		}
	}
}

func (t *h3H2Transport) acquireSessionLocked(hostPort string, allowOverLimit bool) *upstreamSession {
	now := t.currentTime()
	sessions := t.sessions[hostPort]
	kept := sessions[:0]
	var selected *upstreamSession
	for _, session := range sessions {
		if !sessionAvailable(session) {
			if session != nil {
				go session.Close()
			}
			continue
		}
		expired := t.sessionExpired(session, now)
		if expired && session.activeStreams == 0 {
			go session.Close()
			continue
		}
		kept = append(kept, session)
		// A rotating session keeps its existing HTTP or WebSocket streams, but
		// must not accept a new one. sessionFor will establish its replacement.
		if expired {
			continue
		}
		if (allowOverLimit || session.activeStreams < t.maxStreams) && (selected == nil || session.activeStreams < selected.activeStreams) {
			selected = session
		}
	}
	if len(kept) == 0 {
		delete(t.sessions, hostPort)
	} else {
		t.sessions[hostPort] = kept
	}
	if selected != nil {
		selected.activeStreams++
	}
	return selected
}

func sessionAvailable(session *upstreamSession) bool {
	return session != nil && session.available != nil && session.available()
}

func (t *h3H2Transport) releaseSession(hostPort string, session *upstreamSession) func() {
	return func() {
		var closeSession *upstreamSession
		t.mu.Lock()
		if session.activeStreams > 0 {
			session.activeStreams--
		}
		if t.sessionExpired(session, t.currentTime()) && session.activeStreams == 0 {
			if t.removeSessionLocked(hostPort, session) {
				closeSession = session
			}
			if !t.closed {
				t.startDialLocked(hostPort)
			}
		}
		t.mu.Unlock()
		if closeSession != nil {
			closeSession.Close()
		}
	}
}

func (t *h3H2Transport) retainWebSocketCarrier(session *upstreamSession) func() {
	t.mu.Lock()
	session.activeWebSocketCarriers++
	t.mu.Unlock()
	var once sync.Once
	return func() {
		once.Do(func() {
			t.mu.Lock()
			if session.activeWebSocketCarriers > 0 {
				session.activeWebSocketCarriers--
			}
			t.mu.Unlock()
		})
	}
}

func (t *h3H2Transport) startDialLocked(hostPort string) *upstreamDial {
	if dialing := t.dialing[hostPort]; dialing != nil {
		return dialing
	}
	dialContext, cancel := context.WithCancel(context.Background())
	dialing := &upstreamDial{done: make(chan struct{}), cancel: cancel}
	t.dialing[hostPort] = dialing
	go t.raceSessions(dialContext, hostPort, dialing)
	return dialing
}

func (t *h3H2Transport) currentTime() time.Time {
	if t.now == nil {
		return time.Now()
	}
	return t.now()
}

func (t *h3H2Transport) connectionStatusSnapshot() upstreamConnectionStatus {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return upstreamConnectionStatus{}
	}
	status := upstreamConnectionStatus{}
	for _, sessions := range t.sessions {
		for _, session := range sessions {
			if !sessionAvailable(session) {
				continue
			}
			status.activeStreams += session.activeStreams
			status.webSocketCarriers += session.activeWebSocketCarriers
			switch session.protocol {
			case upstreamProtocolH2:
				status.h2Connections++
				status.h2WebSocketCarriers += session.activeWebSocketCarriers
			case upstreamProtocolH3:
				status.h3Connections++
				status.h3WebSocketCarriers += session.activeWebSocketCarriers
			}
		}
	}
	return status
}

func (t *h3H2Transport) sessionExpired(session *upstreamSession, now time.Time) bool {
	return t.sessionLifetime > 0 && !session.createdAt.IsZero() && !now.Before(session.createdAt.Add(t.sessionLifetime))
}

func (t *h3H2Transport) raceSessions(dialContext context.Context, hostPort string, dialing *upstreamDial) {
	t.mu.Lock()
	if t.closed {
		t.mu.Unlock()
		dialing.err = errH3H2TransportClosed
		close(dialing.done)
		return
	}
	t.mu.Unlock()
	ctx, cancel := context.WithTimeout(dialContext, upstreamHandshakeTimeout)
	defer cancel()
	host, port := upstreamHostPort(hostPort)
	ip := net.ParseIP(host)
	if ip == nil || (strings.Contains(host, ":") && !strings.HasPrefix(hostPort, "[")) {
		resolvedIP, err := t.lookupIP(ctx, host)
		if err != nil {
			t.finishDial(hostPort, dialing, nil, err)
			return
		}
		ip = resolvedIP
	}
	address := net.JoinHostPort(ip.String(), port)
	h2Results := make(chan upstreamHandshakeResult, 1)
	h3Results := make(chan upstreamHandshakeResult, 1)
	go func() {
		session, err := t.dialH2(ctx, host, address)
		h2Results <- upstreamHandshakeResult{protocol: upstreamProtocolH2, session: session, err: err}
	}()
	go func() {
		session, err := t.dialH3(ctx, host, address)
		h3Results <- upstreamHandshakeResult{protocol: upstreamProtocolH3, session: session, err: err}
	}()

	var h2Result, h3Result *upstreamHandshakeResult
	var winner *upstreamHandshakeResult
	for winner == nil {
		select {
		case result := <-h3Results:
			h3Result = &result
			if result.err == nil {
				winner = h3Result
			} else if h2Result != nil && h2Result.err != nil {
				winner = h2Result
			}
		case result := <-h2Results:
			h2Result = &result
			if result.err == nil {
				if h3Result != nil {
					winner = h2Result
					continue
				}
				timer := time.NewTimer(t.grace)
				select {
				case result := <-h3Results:
					h3Result = &result
					if result.err == nil {
						winner = h3Result
					} else {
						winner = h2Result
					}
				case <-timer.C:
					winner = h2Result
				case <-ctx.Done():
					winner = &upstreamHandshakeResult{err: context.Cause(ctx)}
				}
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
			} else if h3Result != nil && h3Result.err != nil {
				winner = h2Result
			}
		case <-ctx.Done():
			winner = &upstreamHandshakeResult{err: context.Cause(ctx)}
		}
	}

	cancel()
	t.finishDial(hostPort, dialing, winner.session, winner.err)
	go closeLosingSessions(winner, h2Results, h3Results, h2Result, h3Result)
}

func (t *h3H2Transport) finishDial(hostPort string, dialing *upstreamDial, session *upstreamSession, dialErr error) {
	t.mu.Lock()
	if !t.closed && session != nil && dialErr == nil {
		if session.createdAt.IsZero() {
			session.createdAt = t.currentTime()
		}
		t.sessions[hostPort] = append(t.sessions[hostPort], session)
	} else if session != nil {
		session.Close()
		session = nil
		if dialErr == nil {
			dialErr = errH3H2TransportClosed
		}
	}
	if t.dialing[hostPort] == dialing {
		delete(t.dialing, hostPort)
	}
	dialing.session = session
	dialing.err = dialErr
	close(dialing.done)
	t.mu.Unlock()
}

func upstreamHostPort(hostPort string) (string, string) {
	host, port, err := net.SplitHostPort(hostPort)
	if err == nil {
		return host, port
	}
	if strings.HasPrefix(hostPort, "[") && strings.HasSuffix(hostPort, "]") {
		return strings.TrimSuffix(strings.TrimPrefix(hostPort, "["), "]"), "443"
	}
	return hostPort, "443"
}

func closeLosingSessions(winner *upstreamHandshakeResult, h2Results, h3Results <-chan upstreamHandshakeResult, h2Result, h3Result *upstreamHandshakeResult) {
	if h2Result == nil {
		result := <-h2Results
		h2Result = &result
	}
	if h3Result == nil {
		result := <-h3Results
		h3Result = &result
	}
	for _, result := range []*upstreamHandshakeResult{h2Result, h3Result} {
		if result != nil && result.session != nil && result.session != winner.session {
			result.session.Close()
		}
	}
}

func (t *h3H2Transport) removeSession(hostPort string, session *upstreamSession) {
	t.mu.Lock()
	removed := t.removeSessionLocked(hostPort, session)
	if removed && !t.closed {
		t.startDialLocked(hostPort)
	}
	t.mu.Unlock()
	if removed {
		session.Close()
	}
}

func (t *h3H2Transport) removeSessionLocked(hostPort string, target *upstreamSession) bool {
	sessions := t.sessions[hostPort]
	for index, session := range sessions {
		if session != target {
			continue
		}
		copy(sessions[index:], sessions[index+1:])
		sessions[len(sessions)-1] = nil
		sessions = sessions[:len(sessions)-1]
		if len(sessions) == 0 {
			delete(t.sessions, hostPort)
		} else {
			t.sessions[hostPort] = sessions
		}
		return true
	}
	return false
}

func (t *h3H2Transport) Close() error {
	t.mu.Lock()
	if t.closed {
		t.mu.Unlock()
		return nil
	}
	t.closed = true
	sessions := make([]*upstreamSession, 0)
	for hostPort, hostSessions := range t.sessions {
		sessions = append(sessions, hostSessions...)
		delete(t.sessions, hostPort)
	}
	if t.maintenanceCancel != nil {
		t.maintenanceCancel()
		t.maintenanceCancel = nil
	}
	for hostPort, dialing := range t.dialing {
		if dialing.cancel != nil {
			dialing.cancel()
		}
		delete(t.dialing, hostPort)
	}
	t.mu.Unlock()
	for _, session := range sessions {
		session.Close()
	}
	if closer, ok := t.fallback.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
	return nil
}

func (t *h3H2Transport) CloseIdleConnections() {
	t.mu.Lock()
	for hostPort, sessions := range t.sessions {
		kept := sessions[:0]
		for _, session := range sessions {
			if sessionAvailable(session) {
				kept = append(kept, session)
				continue
			}
			go session.Close()
		}
		if len(kept) == 0 {
			delete(t.sessions, hostPort)
		} else {
			t.sessions[hostPort] = kept
		}
	}
	t.mu.Unlock()
	if closer, ok := t.fallback.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
}

func (g *gateway) settings(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	config := g.currentConfig()
	writeJSON(w, http.StatusOK, settingsResponse{
		ListenAddress:            config.ListenAddress,
		UpstreamBaseURL:          config.UpstreamBaseURL,
		UpstreamAPIKey:           config.UpstreamAPIKey,
		UpstreamWebSocketEnabled: config.UpstreamWebSocketEnabled,
		StartupEnabled:           config.StartupEnabled,
		BackgroundStart:          config.BackgroundStart,
		RetryEnabled:             config.RetryEnabled,
		RetryCount:               config.RetryCount,
		RetryIntervalSeconds:     config.RetryIntervalSeconds,
		RetryStatusCodes:         config.RetryStatusCodes,
	})
}

func (g *gateway) updateSetting(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPut {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	setting := strings.TrimPrefix(r.URL.Path, "/api/settings/")
	if setting == "" || strings.Contains(setting, "/") {
		http.NotFound(w, r)
		return
	}
	var input settingUpdateRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&input); err != nil {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	value := normalizeSettingValue(input.Value)
	if setting == "startup_enabled" || setting == "background_start" {
		enabled, err := strconv.ParseBool(value)
		if err != nil {
			http.Error(w, "value must be true or false", http.StatusBadRequest)
			return
		}
		g.configMu.Lock()
		defer g.configMu.Unlock()
		config := g.config
		if setting == "background_start" && enabled && !config.StartupEnabled {
			http.Error(w, "后台运行必须先开启开机启动", http.StatusBadRequest)
			return
		}
		if setting == "startup_enabled" {
			backgroundEnabled := config.BackgroundStart
			if !enabled {
				backgroundEnabled = false
			}
			if err := syncCodeManagerStartup(enabled); err != nil {
				http.Error(w, "更新开机启动失败: "+err.Error(), http.StatusInternalServerError)
				return
			}
			if err := updateStartupConfig(g.configPath, enabled, backgroundEnabled); err != nil {
				_ = syncCodeManagerStartup(config.StartupEnabled)
				http.Error(w, "save config failed", http.StatusInternalServerError)
				return
			}
			g.config.StartupEnabled = enabled
			g.config.BackgroundStart = backgroundEnabled
			message := "开机启动已关闭。"
			if enabled {
				message = "开机启动已开启。登录到桌面后将等待 8 秒再启动服务。"
			}
			writeJSON(w, http.StatusOK, settingUpdateResponse{Saved: true, Message: message})
			return
		}
		if err := updateConfigBool(g.configPath, "background_start", enabled); err != nil {
			http.Error(w, "save config failed", http.StatusInternalServerError)
			return
		}
		g.config.BackgroundStart = enabled
		message := "后台运行已关闭，开机启动时会打开网页面板。"
		if enabled {
			message = "后台运行已开启，开机启动时不会打开网页面板。"
		}
		writeJSON(w, http.StatusOK, settingUpdateResponse{Saved: true, Message: message})
		return
	}
	if setting == "upstream_base_url" {
		g.updateUpstreamBaseURL(w, value)
		return
	}

	g.configMu.Lock()
	defer g.configMu.Unlock()
	config := g.config
	writeBool := false
	boolValue := false
	switch setting {
	case "listen_address":
		if err := validateListenAddress(value); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		config.ListenAddress = value
	case "upstream_api_key":
		if err := validateUpstreamAPIKey(value); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		config.UpstreamAPIKey = value
	case "retry_enabled":
		enabled, err := strconv.ParseBool(value)
		if err != nil {
			http.Error(w, "value must be true or false", http.StatusBadRequest)
			return
		}
		config.RetryEnabled = enabled
		value = strconv.FormatBool(enabled)
		writeBool = true
		boolValue = enabled
	case "upstream_websocket_enabled":
		enabled, err := strconv.ParseBool(value)
		if err != nil {
			http.Error(w, "value must be true or false", http.StatusBadRequest)
			return
		}
		config.UpstreamWebSocketEnabled = enabled
		value = strconv.FormatBool(enabled)
		writeBool = true
		boolValue = enabled
	case "retry_count":
		value = normalizeRetryCount(value)
		config.RetryCount = value
	case "retry_interval_seconds":
		value = normalizeRetryInterval(value)
		config.RetryIntervalSeconds = value
	case "retry_status_codes":
		value, _ = normalizeRetryStatusCodes(value)
		config.RetryStatusCodes = value
	default:
		http.NotFound(w, r)
		return
	}

	var updateErr error
	if writeBool {
		updateErr = updateConfigBool(g.configPath, setting, boolValue)
	} else {
		updateErr = updateConfigValue(g.configPath, setting, value)
	}
	if updateErr != nil {
		log.Printf("save %s failed: %v", setting, updateErr)
		http.Error(w, "save config failed", http.StatusInternalServerError)
		return
	}
	g.config = config
	message := "已保存，后续请求将立即使用新配置。"
	if setting == "listen_address" {
		message = "已保存到 config.yaml；停止代理后再次启动即可应用新的监听地址。"
	} else if setting == "upstream_websocket_enabled" {
		message = "已保存；后续直连上游请求将按新的 WS 承载协商设置处理。"
	}
	writeJSON(w, http.StatusOK, settingUpdateResponse{Saved: true, Message: message, Value: value})
}

// updateUpstreamBaseURL takes the llmtrim lifecycle lock before the gateway
// config lock. This keeps a concurrent uninstall from deleting the managed
// config directory and then having an in-flight URL save recreate it.
func (g *gateway) updateUpstreamBaseURL(w http.ResponseWriter, value string) {
	value = normalizeUpstreamBaseURL(value)
	if err := validateUpstreamBaseURL(value); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	g.llmtrimMu.Lock()
	defer g.llmtrimMu.Unlock()
	g.configMu.Lock()
	defer g.configMu.Unlock()
	if err := saveUpstreamBaseURLWithLLMTrimConfigForExecutable(g.configPath, value, g.config.LLMTrimPath); err != nil {
		log.Printf("save upstream_base_url failed: %v", err)
		http.Error(w, "save config failed", http.StatusInternalServerError)
		return
	}
	g.config.UpstreamBaseURL = value
	writeJSON(w, http.StatusOK, settingUpdateResponse{
		Saved:           true,
		RequiresRestart: true,
		Message:         "已保存到 config.yaml，并接管同步 llmtrim extra_hosts；停止后再次启动 llmtrim 才会使用新的主机和 CA。",
		Value:           value,
	})
}

func (g *gateway) currentConfig() Config {
	g.configMu.RLock()
	defer g.configMu.RUnlock()
	return g.config
}

func normalizeUpstreamBaseURL(rawURL string) string {
	value := normalizeSettingValue(rawURL)
	if value == "" {
		return ""
	}
	parsed, err := url.ParseRequestURI(value)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" {
		return value
	}
	trimmedPath := strings.TrimRight(parsed.Path, "/")
	if trimmedPath == parsed.Path {
		return value
	}
	parsed.Path = trimmedPath
	parsed.RawPath = ""
	return parsed.String()
}

func normalizeSettingValue(value string) string {
	value = strings.NewReplacer("\r", "", "\n", "").Replace(value)
	return strings.TrimSpace(value)
}

func (g *gateway) currentClient() *http.Client {
	g.configMu.RLock()
	defer g.configMu.RUnlock()
	return g.client
}

func (g *gateway) selectUpstreamClient(llmtrimRunning bool) (*http.Client, error) {
	if !llmtrimRunning {
		g.closeLLMTrimProxyClientIdleConnections()
		client := g.currentClient()
		if client == nil {
			return nil, errors.New("HTTP 客户端尚未就绪")
		}
		return client, nil
	}

	g.llmtrimProxyMu.Lock()
	defer g.llmtrimProxyMu.Unlock()
	if g.llmtrimProxyClient != nil {
		return g.llmtrimProxyClient, nil
	}
	factory := g.llmtrimProxyFactory
	if factory == nil {
		factory = newLLMTrimProxyClient
	}
	client, err := factory()
	if err != nil {
		return nil, fmt.Errorf("create llmtrim proxy client: %w", err)
	}
	g.llmtrimProxyClient = client
	return client, nil
}

func (g *gateway) closeLLMTrimProxyClientIdleConnections() {
	g.llmtrimProxyMu.Lock()
	client := g.llmtrimProxyClient
	g.llmtrimProxyClient = nil
	g.llmtrimProxyMu.Unlock()
	closeHTTPClientTransport(client)
}

func clientTransport(client *http.Client) http.RoundTripper {
	if client == nil {
		return nil
	}
	if client.Transport != nil {
		return client.Transport
	}
	return http.DefaultTransport
}

func closeHTTPClientTransport(client *http.Client) {
	if client == nil || client.Transport == nil {
		return
	}
	if closer, ok := client.Transport.(interface{ Close() error }); ok {
		if err := closer.Close(); err != nil {
			log.Printf("close outbound transport failed: %v", err)
		}
		return
	}
	if closer, ok := client.Transport.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
}

func (g *gateway) closeHTTPClient() {
	g.configMu.Lock()
	client := g.client
	g.client = nil
	g.configMu.Unlock()
	closeHTTPClientTransport(client)
}

func (g *gateway) loadProxyConfiguration() (Config, *http.Client, error) {
	config, err := loadConfig(g.configPath)
	if err != nil {
		return Config{}, nil, err
	}
	client, err := makeHTTPClient(config.OutboundProxy)
	if err != nil {
		return Config{}, nil, fmt.Errorf("create outbound client: %w", err)
	}
	return config, client, nil
}

func (g *gateway) isProxyRunning() bool {
	g.proxyMu.RLock()
	defer g.proxyMu.RUnlock()
	return g.proxyRunning
}

func (g *gateway) proxyStatusSnapshot() (bool, string, string, string) {
	g.proxyMu.RLock()
	defer g.proxyMu.RUnlock()
	return g.proxyRunning, g.proxyState, g.proxyAddress, g.proxyMessage
}

func (g *gateway) beginLocalHTTP1Request() func() {
	g.localHTTP1Requests.Add(1)
	var once sync.Once
	return func() {
		once.Do(func() {
			g.localHTTP1Requests.Add(-1)
		})
	}
}

func (g *gateway) connectionStatusSnapshot() connectionStatusResponse {
	status := connectionStatusResponse{LocalHTTP1: g.localHTTP1Requests.Load()}
	g.webSocketMu.Lock()
	status.LocalWebSocket = len(g.webSocketSessions)
	g.webSocketMu.Unlock()

	g.configMu.RLock()
	client := g.client
	g.configMu.RUnlock()
	transport, ok := clientTransport(client).(*h3H2Transport)
	if !ok {
		return status
	}
	upstream := transport.connectionStatusSnapshot()
	status.UpstreamH2 = upstream.h2Connections
	status.UpstreamH2WebSocket = upstream.h2WebSocketCarriers > 0
	status.UpstreamH3 = upstream.h3Connections
	status.UpstreamH3WebSocket = upstream.h3WebSocketCarriers > 0
	status.UpstreamWebSocket = upstream.webSocketCarriers
	status.UpstreamStreams = upstream.activeStreams
	return status
}

func (g *gateway) isShuttingDown() bool {
	g.proxyMu.RLock()
	defer g.proxyMu.RUnlock()
	return g.shuttingDown
}

func (g *gateway) proxyListenAddress() string {
	g.proxyMu.RLock()
	defer g.proxyMu.RUnlock()
	return g.proxyAddress
}

func (g *gateway) beginShutdown() {
	g.proxyMu.Lock()
	g.shuttingDown = true
	g.proxyMu.Unlock()
	log.Print("shutdown: proxy forwarding is draining")
}

func (g *gateway) waitProxyRequests(ctx context.Context) error {
	done := make(chan struct{})
	go func() {
		g.proxyRequests.Wait()
		close(done)
	}()
	select {
	case <-done:
		log.Print("shutdown: proxy requests drained")
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (g *gateway) proxyConnectionContext(ctx context.Context, connection net.Conn) context.Context {
	return context.WithValue(ctx, proxyConnectionContextKey{}, connection)
}

func (g *gateway) proxyConnectionState(connection net.Conn, state http.ConnState) {
	if state == http.StateClosed {
		g.unregisterProxyConnection(connection)
	}
}

func (g *gateway) registerProxyConnection(ctx context.Context) {
	connection, _ := ctx.Value(proxyConnectionContextKey{}).(net.Conn)
	if connection == nil {
		return
	}
	g.proxyConnectionMu.Lock()
	if g.proxyConnections == nil {
		g.proxyConnections = make(map[net.Conn]struct{})
	}
	g.proxyConnections[connection] = struct{}{}
	g.proxyConnectionMu.Unlock()
}

func (g *gateway) unregisterProxyConnection(connection net.Conn) {
	if connection == nil {
		return
	}
	g.proxyConnectionMu.Lock()
	delete(g.proxyConnections, connection)
	g.proxyConnectionMu.Unlock()
}

func (g *gateway) closeProxyConnections() {
	g.proxyConnectionMu.Lock()
	connections := make([]net.Conn, 0, len(g.proxyConnections))
	for connection := range g.proxyConnections {
		connections = append(connections, connection)
	}
	g.proxyConnections = make(map[net.Conn]struct{})
	g.proxyConnectionMu.Unlock()
	for _, connection := range connections {
		_ = connection.Close()
	}
}

func (g *gateway) closeGatewayProxyConnections() {
	g.closeWebSocketSessions()
	g.closeProxyConnections()
}

func (g *gateway) enterProxyRequest(ctx context.Context) (context.Context, func(), bool) {
	for {
		g.proxyMu.Lock()
		if g.shuttingDown {
			g.proxyMu.Unlock()
			return nil, nil, false
		}
		if g.proxyRunning {
			runContext := g.proxyRunContext
			g.proxyRequests.Add(1)
			g.proxyMu.Unlock()
			requestContext, cancel := context.WithCancel(ctx)
			stopCancel := func() func() { return func() {} }
			if runContext != nil {
				stopCancel = func() func() {
					remove := context.AfterFunc(runContext, cancel)
					return func() { _ = remove() }
				}
			}
			stop := stopCancel()
			return requestContext, func() {
				stop()
				cancel()
				g.proxyRequests.Done()
			}, true
		}
		if g.proxyState != proxyStateConnecting || g.proxyReady == nil {
			g.proxyMu.Unlock()
			return nil, nil, false
		}
		ready := g.proxyReady
		g.proxyMu.Unlock()
		select {
		case <-ready:
		case <-ctx.Done():
			return nil, nil, false
		}
	}
}

func (g *gateway) proxyRequestUnavailable() (string, string) {
	g.proxyMu.RLock()
	defer g.proxyMu.RUnlock()
	state := g.proxyState
	message := g.proxyMessage
	if state == "" {
		state = proxyStateStopped
	}
	if message == "" {
		message = "代理转发已停止，请在网页中点击“启动代理”。"
	}
	return state, message
}

func (g *gateway) leaveProxyRequest() {
	g.proxyRequests.Done()
}

func (g *gateway) proxyStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	config := g.currentConfig()
	running, state, listenAddress, message := g.proxyStatusSnapshot()
	if message == "" {
		message = "HTTP 代理转发已启动。"
	}
	processID := os.Getpid()
	if state == proxyStateStopped {
		message = "代理转发已停止，HTTP /v1 请求会返回 503。"
		listenAddress = config.ListenAddress
	}
	writeJSON(w, http.StatusOK, proxyStatusResponse{Running: running, State: state, ProcessID: processID, ListenAddress: listenAddress, Message: message, Connections: g.connectionStatusSnapshot()})
}

func (g *gateway) managementForward(w http.ResponseWriter, r *http.Request) {
	g.proxyMu.RLock()
	serveProxy := sameListenAddress(g.proxyAddress, g.managementAddress) && (g.proxyRunning || g.proxyState == proxyStateConnecting)
	state := g.proxyState
	message := g.proxyMessage
	g.proxyMu.RUnlock()
	if serveProxy {
		g.forward(w, r)
		return
	}
	config := g.currentConfig()
	status := http.StatusServiceUnavailable
	if state == proxyStateUnavailable {
		status = http.StatusBadGateway
	}
	if message == "" {
		message = "代理转发已停止，请在网页中点击“启动代理”。"
	}
	writeJSON(w, status, proxyStatusResponse{
		Running:       false,
		State:         state,
		ListenAddress: config.ListenAddress,
		Message:       message,
		Connections:   g.connectionStatusSnapshot(),
	})
}

func (g *gateway) startProxy() error {
	g.proxyMu.Lock()
	if g.shuttingDown {
		g.proxyMu.Unlock()
		return errors.New("code-Manager 正在退出，代理控制已停止")
	}
	if g.proxyRunning || g.proxyState == proxyStateConnecting {
		g.proxyMu.Unlock()
		return errors.New("代理已经运行，请先停止代理")
	}
	g.proxyGeneration++
	generation := g.proxyGeneration
	g.proxyState = proxyStateConnecting
	g.proxyMessage = "正在建立上游 H2/H3 TLS 连接，并准备扩展 CONNECT。"
	g.proxyReady = make(chan struct{})
	g.proxyReadyClosed = false
	g.proxyReadyErr = nil
	startContext, startCancel := context.WithCancel(context.Background())
	runContext, runCancel := context.WithCancel(context.Background())
	g.proxyStartCancel = startCancel
	g.proxyRunContext = runContext
	g.proxyRunCancel = runCancel
	g.proxyMu.Unlock()

	config, client, err := g.loadProxyConfiguration()
	if err != nil {
		startCancel()
		runCancel()
		g.finishProxyStartFailure(generation, fmt.Errorf("reload config: %w", err), false)
		return fmt.Errorf("reload config: %w", err)
	}
	if err := validateProxyConfiguration(config); err != nil {
		startCancel()
		runCancel()
		closeHTTPClientTransport(client)
		g.finishProxyStartFailure(generation, err, false)
		return err
	}
	g.proxyMu.Lock()
	if g.proxyGeneration != generation || g.shuttingDown || startContext.Err() != nil {
		g.proxyMu.Unlock()
		closeHTTPClientTransport(client)
		return errors.New("代理启动已取消")
	}
	g.configMu.Lock()
	previousClient := g.client
	g.config = config
	g.client = client
	g.configMu.Unlock()
	g.proxyMu.Unlock()
	closeHTTPClientTransport(previousClient)

	var server *http.Server
	var listener net.Listener
	listenAddress := g.managementAddress
	if sameListenAddress(config.ListenAddress, g.managementAddress) {
		g.proxyMu.Lock()
		if g.proxyGeneration == generation {
			g.proxyAddress = g.managementAddress
		}
		g.proxyMu.Unlock()
	} else {
		listener, err = net.Listen("tcp", config.ListenAddress)
		if err != nil {
			startCancel()
			runCancel()
			closeHTTPClientTransport(client)
			g.finishProxyStartFailure(generation, fmt.Errorf("listen on %s: %w", config.ListenAddress, err), false)
			return fmt.Errorf("listen on %s: %w", config.ListenAddress, err)
		}
		listenAddress = listener.Addr().String()
		server = &http.Server{
			Addr:              listenAddress,
			Handler:           g.proxyHandler,
			ReadHeaderTimeout: 10 * time.Second,
			ConnContext:       g.proxyConnectionContext,
			ConnState:         g.proxyConnectionState,
		}
		g.proxyMu.Lock()
		if g.proxyGeneration != generation || g.shuttingDown || startContext.Err() != nil {
			g.proxyMu.Unlock()
			_ = listener.Close()
			closeHTTPClientTransport(client)
			return errors.New("代理启动已取消")
		}
		g.proxyAddress = listenAddress
		g.proxyServer = server
		g.proxyMu.Unlock()
		go g.serveProxy(server, listener)
	}

	if transport, ok := clientTransport(client).(*h3H2Transport); ok {
		err = prewarmUpstreamWithRetry(startContext, transport, config.UpstreamBaseURL)
		if err != nil {
			startCancel()
			runCancel()
			closeHTTPClientTransport(client)
			g.finishProxyStartFailure(generation, err, listener != nil)
			closeFailedProxyListener(server, listener)
			return err
		}
		transport.StartMaintenance(runContext)
	}

	g.proxyMu.Lock()
	if g.proxyGeneration != generation || g.shuttingDown || startContext.Err() != nil {
		g.proxyMu.Unlock()
		startCancel()
		runCancel()
		closeHTTPClientTransport(client)
		closeFailedProxyListener(server, listener)
		return errors.New("代理启动已取消")
	}
	g.proxyRunning = true
	g.proxyState = proxyStateRunning
	g.proxyMessage = "HTTP/WS 网关已启动，上游 H2/H3 扩展 CONNECT 已就绪。"
	g.proxyReadyErr = nil
	g.proxyStartCancel = nil
	if !g.proxyReadyClosed {
		close(g.proxyReady)
		g.proxyReadyClosed = true
	}
	g.proxyMu.Unlock()
	startCancel()
	log.Printf("HTTP forwarding started on %s", listenAddress)
	return nil
}

func closeFailedProxyListener(server *http.Server, listener net.Listener) {
	if server == nil {
		if listener != nil {
			_ = listener.Close()
		}
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := server.Shutdown(ctx); err != nil {
		_ = server.Close()
	}
}

func (g *gateway) finishProxyStartFailure(generation uint64, startErr error, dedicatedListener bool) {
	g.proxyMu.Lock()
	defer g.proxyMu.Unlock()
	if g.proxyGeneration != generation {
		return
	}
	g.proxyRunning = false
	g.proxyServer = nil
	if dedicatedListener {
		g.proxyAddress = ""
	}
	g.proxyState = proxyStateUnavailable
	g.proxyMessage = "上游不可用：" + startErr.Error()
	g.proxyReadyErr = startErr
	g.proxyStartCancel = nil
	g.proxyRunContext = nil
	g.proxyRunCancel = nil
	if !g.proxyReadyClosed {
		close(g.proxyReady)
		g.proxyReadyClosed = true
	}
}

func prewarmUpstreamWithRetry(ctx context.Context, transport *h3H2Transport, rawURL string) error {
	var lastErr error
	for attempt := 1; attempt <= upstreamHandshakeAttempts; attempt++ {
		if err := transport.Prewarm(ctx, rawURL); err == nil {
			return nil
		} else if ctx.Err() != nil {
			return context.Cause(ctx)
		} else {
			lastErr = err
			log.Printf("upstream H2/H3 handshake attempt %d/%d failed: %v", attempt, upstreamHandshakeAttempts, err)
		}
		if attempt == upstreamHandshakeAttempts {
			break
		}
		timer := time.NewTimer(upstreamHandshakeRetryDelay)
		select {
		case <-timer.C:
		case <-ctx.Done():
			if !timer.Stop() {
				select {
				case <-timer.C:
				default:
				}
			}
			return context.Cause(ctx)
		}
	}
	return fmt.Errorf("连续 %d 次 H2/H3 握手失败: %w", upstreamHandshakeAttempts, lastErr)
}

func (g *gateway) serveProxy(server *http.Server, listener net.Listener) {
	err := server.Serve(listener)
	if err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Printf("proxy listener stopped unexpectedly: %v", err)
	}
	closeClient := false
	g.proxyMu.Lock()
	if g.proxyServer == server {
		g.proxyServer = nil
		g.proxyAddress = ""
		g.proxyRunning = false
		if g.proxyState != proxyStateUnavailable {
			g.proxyState = proxyStateStopped
			g.proxyMessage = "HTTP 代理监听已停止。"
		}
		if g.proxyRunCancel != nil {
			g.proxyRunCancel()
			g.proxyRunCancel = nil
		}
		closeClient = true
	}
	g.proxyMu.Unlock()
	if closeClient {
		g.closeGatewayProxyConnections()
		g.configMu.RLock()
		client := g.client
		g.configMu.RUnlock()
		closeHTTPClientTransport(client)
		g.closeLLMTrimProxyClientIdleConnections()
	}
}

func (g *gateway) proxyStart(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.startProxy(); err != nil {
		log.Printf("start proxy failed: %v", err)
		writeJSON(w, http.StatusBadGateway, proxyStatusResponse{
			Running:       false,
			State:         proxyStateUnavailable,
			ListenAddress: g.currentConfig().ListenAddress,
			Message:       "启动代理失败: " + err.Error(),
			Connections:   g.connectionStatusSnapshot(),
		})
		return
	}
	running, state, address, message := g.proxyStatusSnapshot()
	writeJSON(w, http.StatusOK, proxyStatusResponse{Running: running, State: state, ProcessID: os.Getpid(), ListenAddress: address, Message: message, Connections: g.connectionStatusSnapshot()})
}

func (g *gateway) proxyStop(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := g.stopProxy(ctx); err != nil {
		log.Printf("stop proxy failed: %v", err)
		http.Error(w, "停止代理失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	config := g.currentConfig()
	writeJSON(w, http.StatusOK, proxyStatusResponse{Running: false, State: proxyStateStopped, ProcessID: os.Getpid(), ListenAddress: config.ListenAddress, Message: "代理已彻底停止；code-Manager 管理页面仍保持运行。", Connections: g.connectionStatusSnapshot()})
}

func (g *gateway) applicationExit(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if g.application == nil {
		http.Error(w, "code-Manager 退出控制不可用", http.StatusServiceUnavailable)
		return
	}
	result := g.application.shutdownManagedResources()
	response := applicationExitResponse{
		Completed: true,
		Clean:     len(result.Warnings) == 0,
		Warnings:  result.Warnings,
	}
	if response.Clean {
		response.Message = "已停止并退出 code-Manager。"
	} else {
		response.Message = "code-Manager 将退出，但部分清理未完成。"
	}
	writeJSON(w, http.StatusOK, response)
	if flusher, ok := w.(http.Flusher); ok {
		flusher.Flush()
	}
	go func(app *application) {
		app.shutdown()
		systray.Quit()
	}(g.application)
}

// applicationIdentity lets the release uninstaller verify that port 7780 is
// served by the EXE in its own directory before it asks that instance to exit.
func (g *gateway) applicationIdentity(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	executable, err := os.Executable()
	if err != nil {
		http.Error(w, "无法定位 code-Manager.exe", http.StatusInternalServerError)
		return
	}
	executable, err = filepath.Abs(executable)
	if err != nil {
		http.Error(w, "无法规范化 code-Manager.exe 路径", http.StatusInternalServerError)
		return
	}
	version, err := embeddedApplicationVersion()
	if err != nil {
		log.Printf("读取内置 vision.md 失败: %v", err)
		http.Error(w, "无法读取内置版本信息", http.StatusInternalServerError)
		return
	}
	writeJSON(w, http.StatusOK, applicationIdentityResponse{ExecutablePath: executable, Version: version})
}

func (g *gateway) stopProxy(ctx context.Context) error {
	g.proxyMu.Lock()
	g.proxyRunning = false
	g.proxyState = proxyStateStopped
	g.proxyMessage = "代理正在停止。"
	proxyServer := g.proxyServer
	g.proxyServer = nil
	g.proxyAddress = ""
	g.proxyGeneration++
	if g.proxyStartCancel != nil {
		g.proxyStartCancel()
		g.proxyStartCancel = nil
	}
	if g.proxyRunCancel != nil {
		g.proxyRunCancel()
		g.proxyRunCancel = nil
	}
	ready := g.proxyReady
	if ready != nil && !g.proxyReadyClosed {
		close(ready)
		g.proxyReadyClosed = true
	}
	g.proxyMu.Unlock()

	// http.Server.Close does not close hijacked WebSocket connections.
	g.closeGatewayProxyConnections()

	// 先关闭上游连接，立即打断正在握手或传输中的请求；专用监听也立即关闭。
	// 管理页共用监听时不能关闭管理服务，但 /v1 请求已由上游关闭打断。
	g.configMu.RLock()
	client := g.client
	g.configMu.RUnlock()
	closeHTTPClientTransport(client)
	g.closeLLMTrimProxyClientIdleConnections()
	var stopErr error
	if proxyServer != nil {
		if err := proxyServer.Close(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			stopErr = fmt.Errorf("close proxy listener: %w", err)
		}
	}
	if err := g.waitProxyRequests(ctx); err != nil {
		if stopErr == nil {
			stopErr = fmt.Errorf("wait proxy requests: %w", err)
		}
	}
	log.Print("proxy forwarding stopped")
	return stopErr
}

func (g *gateway) stopLLMTrim(ctx context.Context) error {
	g.llmtrimMu.Lock()
	defer g.llmtrimMu.Unlock()

	config := g.currentConfig()
	if err := g.stopLLMTrimAndCleanup(ctx, config.LLMTrimPath); err != nil {
		return err
	}
	log.Print("shutdown: llmtrim stopped and confirmed")
	return nil
}

func (g *gateway) observeLLMTrimRunning(executable string) bool {
	if g.llmtrimRunningCheck != nil {
		return g.llmtrimRunningCheck(executable)
	}
	running, _, _, err := queryLLMTrimRunning(executable)
	return err == nil && running
}

func (g *gateway) logStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	showing := g.logViewer.running()
	message := "日志窗口已关闭。"
	if showing {
		message = "日志窗口正在显示。"
	}
	writeJSON(w, http.StatusOK, logStatusResponse{Showing: showing, Message: message})
}

func (g *gateway) showLogs(w http.ResponseWriter, r *http.Request) {
	g.setLogViewerState(w, r, true)
}

func (g *gateway) hideLogs(w http.ResponseWriter, r *http.Request) {
	g.setLogViewerState(w, r, false)
}

func (g *gateway) llmtrimLogStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	showing := g.llmtrimStatusViewer.running()
	message := "llmtrim CMD 窗口已关闭。"
	if showing {
		message = "llmtrim CMD 窗口正在显示。"
	}
	writeJSON(w, http.StatusOK, logStatusResponse{Showing: showing, Message: message})
}

func (g *gateway) showLLMTrimLogs(w http.ResponseWriter, r *http.Request) {
	g.setLLMTrimLogViewerState(w, r, true)
}

func (g *gateway) hideLLMTrimLogs(w http.ResponseWriter, r *http.Request) {
	g.setLLMTrimLogViewerState(w, r, false)
}

func (g *gateway) setLLMTrimLogViewerState(w http.ResponseWriter, r *http.Request, showing bool) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if showing && g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，llmtrim CMD 窗口不能打开", http.StatusServiceUnavailable)
		return
	}
	var err error
	if showing {
		err = g.llmtrimStatusViewer.start(g.currentConfig().LLMTrimPath)
	} else {
		err = g.llmtrimStatusViewer.stop()
	}
	if err != nil {
		log.Printf("set llmtrim status viewer showing=%t failed: %v", showing, err)
		http.Error(w, "llmtrim CMD 窗口操作失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	message := "llmtrim CMD 窗口已关闭。"
	if showing {
		message = "llmtrim CMD 窗口已打开，正在运行 .\\llmtrim.exe status。"
	}
	writeJSON(w, http.StatusOK, logStatusResponse{Showing: g.llmtrimStatusViewer.running(), Message: message})
}

func (g *gateway) setLogViewerState(w http.ResponseWriter, r *http.Request, showing bool) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if showing && g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，日志窗口不能打开", http.StatusServiceUnavailable)
		return
	}
	var err error
	if showing {
		err = g.logViewer.start(g.logPath)
	} else {
		err = g.logViewer.stop()
	}
	if err != nil {
		log.Printf("set log viewer showing=%t failed: %v", showing, err)
		http.Error(w, "日志窗口操作失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	message := "日志窗口已关闭。"
	if showing {
		message = "日志窗口已打开，正在镜像当前会话日志。"
	}
	writeJSON(w, http.StatusOK, logStatusResponse{Showing: g.logViewer.running(), Message: message})
}

func (g *gateway) llmtrimStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	config := g.currentConfig()
	configuredPath := strings.TrimSpace(config.LLMTrimPath)
	running, processID, message, runningErr := queryLLMTrimRunning(configuredPath)
	installDir, installErr := llmtrimInstallDirectory()
	directoryExists := false
	var toolState managedToolState
	var stateErr error
	if installErr == nil {
		if info, statErr := os.Stat(installDir); statErr == nil {
			directoryExists = info.IsDir()
		}
		toolState, stateErr = readManagedToolState(installDir)
	}
	configuredPathAvailable := llmtrimExecutableExists(configuredPath)
	installed := configuredPathAvailable
	if installErr == nil {
		installed = installed || llmtrimExecutableExists(filepath.Join(installDir, llmtrimExecutableName))
	}
	version := ""
	if installErr == nil {
		version = currentLLMTrimVersion(installDir, configuredPath)
	} else if configuredPathAvailable {
		version = queryLLMTrimBinaryVersion(configuredPath)
	}
	stateDirExists := false
	if userProfile := strings.TrimSpace(os.Getenv("USERPROFILE")); userProfile != "" {
		if info, statErr := os.Stat(filepath.Join(userProfile, ".llmtrim")); statErr == nil {
			stateDirExists = info.IsDir()
		}
	}
	allProcesses := findRunningProcessesByNames(llmtrimExecutableName, llmtrimTrayExecutableName)
	llmtrimProcessRunning := false
	if processID == 0 {
		for _, process := range allProcesses {
			if strings.EqualFold(filepath.Base(process.Path), llmtrimExecutableName) {
				llmtrimProcessRunning = true
				processID = process.ID
				if running {
					break
				}
			}
		}
	}
	if !llmtrimProcessRunning {
		for _, process := range allProcesses {
			if strings.EqualFold(filepath.Base(process.Path), llmtrimExecutableName) {
				llmtrimProcessRunning = true
				if processID == 0 {
					processID = process.ID
				}
				break
			}
		}
	}
	if !running && llmtrimProcessRunning {
		message += fmt.Sprintf("；检测到 llmtrim.exe 进程（PID %d），但尚未确认当前配置路径的 daemon 已就绪", processID)
	}
	trayProcesses := findRunningProcessesByNames(llmtrimTrayExecutableName)
	trayRunning := len(trayProcesses) > 0
	trayProcessID := 0
	if trayRunning {
		trayProcessID = trayProcesses[0].ID
	}
	portOpen := isTCPPortOpen(llmtrimDaemonAddress)
	residual, residualErr := queryLLMTrimResidual(configuredPath, installDir, running, trayRunning, directoryExists, stateDirExists)
	if residualErr != nil {
		residual = true
		message += "；读取 llmtrim 残留状态失败: " + residualErr.Error()
	}
	if runningErr != nil {
		message += "；读取 daemon 状态失败: " + runningErr.Error()
	}
	if installErr != nil {
		message += "；定位受管安装目录失败: " + installErr.Error()
	}
	if stateErr != nil {
		message += "；读取 llmtrim 状态账本失败: " + stateErr.Error()
	}
	if trayRunning {
		message += fmt.Sprintf("；llmtrim-tray.exe 正在运行（PID %d）", trayProcessID)
	}
	configured, configurationMessage := verifyLLMTrimWindowsSetup()
	activationState := llmtrimActivationState(llmtrimActivationSnapshot{
		Installed:               installed,
		DirectoryExists:         directoryExists,
		ConfiguredPath:          configuredPath,
		ConfiguredPathAvailable: configuredPathAvailable,
		Running:                 running,
		TrayRunning:             trayRunning,
		ProcessID:               processID,
		PortOpen:                portOpen,
		DesiredRunning:          toolState.DesiredRunning,
		RecordedRunning:         toolState.Running,
		WindowsConfigured:       configured,
		StateReadFailed:         installErr != nil || stateErr != nil,
		StateNeedsAttention:     managedToolStateHasAttention(toolState),
	})
	if activationState == "attention" {
		switch {
		case running && !configured:
			message += "；daemon 已运行，但 Windows 接管未通过校验: " + configurationMessage
		case !running && trayRunning:
			message += "；daemon 已停止但 tray 仍在运行，请点击停止清理。"
		case !running && portOpen:
			message += "；43117 端口仍被占用，当前配置路径的 daemon 未确认就绪。"
		case configuredPath == "":
			message += "；未记录可启动的 llmtrim.exe 路径，请重新安装。"
		case !configuredPathAvailable:
			message += "；已记录的 llmtrim.exe 路径不可用，请重新安装。"
		case !running && (toolState.DesiredRunning || toolState.Running):
			message += "；状态账本要求 llmtrim 运行，但 daemon 尚未就绪。"
		case managedToolStateHasAttention(toolState):
			message += "；上次 llmtrim 操作需要处理: " + strings.TrimSpace(toolState.Metadata["last_error"])
		}
	}
	writeJSON(w, http.StatusOK, llmtrimStatusResponse{Path: configuredPath, Version: version, Running: running, DesiredRunning: toolState.DesiredRunning, ActivationState: activationState, Installed: installed, Configured: configured, DirectoryExists: directoryExists, StateDirExists: stateDirExists, TrayRunning: trayRunning, ProcessID: processID, TrayProcessID: trayProcessID, Residual: residual, Port: llmtrimDaemonAddress, Message: message})
}

func (g *gateway) llmtrimStart(w http.ResponseWriter, r *http.Request) {
	// setup 会同时恢复 Windows 环境接管并启动 daemon。
	g.runLLMTrimCommand(w, r, "setup")
}

func (g *gateway) llmtrimStop(w http.ResponseWriter, r *http.Request) {
	g.runLLMTrimCommand(w, r, "stop")
}

func (g *gateway) markLLMTrimAttention(cause error) {
	if cause == nil {
		return
	}
	installDir, err := llmtrimInstallDirectory()
	if err != nil {
		log.Printf("无法记录 llmtrim 异常状态: %v", err)
		return
	}
	info, err := os.Stat(installDir)
	if errors.Is(err, os.ErrNotExist) || (err == nil && !info.IsDir()) {
		return
	}
	if err != nil {
		log.Printf("无法检查 llmtrim 安装目录以记录异常: %v", err)
		return
	}
	state, err := readManagedToolState(installDir)
	if err != nil {
		log.Printf("无法读取 llmtrim 状态账本以记录异常: %v", err)
		return
	}
	markManagedToolAttention(installDir, state, cause)
}

func (g *gateway) runLLMTrimCommand(w http.ResponseWriter, r *http.Request, action string) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := g.llmtrimStatusViewer.stop(); err != nil {
		http.Error(w, "关闭 llmtrim CMD 窗口失败: "+err.Error(), http.StatusBadGateway)
		return
	}
	if g.isShuttingDown() {
		http.Error(w, "code-Manager 正在退出，llmtrim 控制已停止", http.StatusServiceUnavailable)
		return
	}

	var input llmtrimCommandRequest
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8<<10)).Decode(&input); err != nil {
		http.Error(w, "invalid JSON request", http.StatusBadRequest)
		return
	}
	// 页面不再允许编辑 llmtrim_path。保留旧字段仅为兼容已有调用方，
	// 生命周期操作始终使用服务端已保存的配置路径。
	pathValue := strings.TrimSpace(g.currentConfig().LLMTrimPath)
	if action == "stop" {
		g.llmtrimMu.Lock()
		defer g.llmtrimMu.Unlock()
		validatedPath := pathValue
		if strings.TrimSpace(validatedPath) != "" {
			if normalizedPath, validateErr := validateLLMTrimPath(validatedPath); validateErr == nil {
				validatedPath = normalizedPath
			} else {
				log.Printf("llmtrim stop path is unavailable; leaving unknown processes untouched: %v", validateErr)
				validatedPath = ""
			}
		}
		if err := g.stopLLMTrimAndCleanup(r.Context(), validatedPath); err != nil {
			g.markLLMTrimAttention(err)
			http.Error(w, "llmtrim 和 llmtrim-tray 停止失败: "+err.Error(), http.StatusBadGateway)
			return
		}
		if installDir, dirErr := llmtrimInstallDirectory(); dirErr == nil {
			if info, statErr := os.Stat(installDir); statErr == nil && info.IsDir() {
				if stateErr := writeManagedToolState(installDir, managedToolState{DesiredRunning: false, Running: false}); stateErr != nil {
					log.Printf("保存 llmtrim 停止状态失败: %v", stateErr)
				}
			}
		}
		configured, _ := verifyLLMTrimWindowsSetup()
		installDir, _ := llmtrimInstallDirectory()
		writeJSON(w, http.StatusOK, llmtrimStatusResponse{Path: validatedPath, Version: currentLLMTrimVersion(installDir, validatedPath), Running: false, DesiredRunning: false, ActivationState: "installed_stopped", Configured: configured, Port: llmtrimDaemonAddress, Message: "llmtrim 和 llmtrim-tray 已停止，相关自启动和用户环境变量已清理。"})
		return
	}
	if action != "setup" {
		http.Error(w, "unsupported llmtrim action", http.StatusBadRequest)
		return
	}
	pathValue, err := validateLLMTrimPath(pathValue)
	if err != nil {
		g.markLLMTrimAttention(err)
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if err := g.startLLMTrimWithPath(r.Context(), pathValue); err != nil {
		log.Printf("llmtrim setup failed: %v", err)
		g.markLLMTrimAttention(err)
		http.Error(w, "llmtrim setup failed: "+err.Error(), http.StatusBadGateway)
		return
	}
	_, processID, _, _ := queryLLMTrimRunning(pathValue)
	configured, _ := verifyLLMTrimWindowsSetup()
	installDir, _ := llmtrimInstallDirectory()
	writeJSON(w, http.StatusOK, llmtrimStatusResponse{Path: pathValue, Version: currentLLMTrimVersion(installDir, pathValue), Running: true, DesiredRunning: true, ActivationState: "running", Configured: configured, ProcessID: processID, Port: llmtrimDaemonAddress, Message: "llmtrim 已启动并确认正在运行。"})
}

func (g *gateway) startLLMTrim(ctx context.Context) error {
	config := g.currentConfig()
	pathValue, err := validateLLMTrimPath(config.LLMTrimPath)
	if err != nil {
		return err
	}
	return g.startLLMTrimWithPath(ctx, pathValue)
}

func (g *gateway) startLLMTrimWithPath(ctx context.Context, pathValue string) error {
	g.llmtrimMu.Lock()
	defer g.llmtrimMu.Unlock()
	if g.isShuttingDown() {
		return errors.New("code-Manager 正在退出，llmtrim 控制已停止")
	}
	// setup may recreate the local CA. Drop a client retained from a daemon
	// crash or an external restart so the next request rebuilds its trust pool.
	g.closeLLMTrimProxyClientIdleConnections()
	if err := g.persistLLMTrimPath(pathValue); err != nil {
		return fmt.Errorf("save llmtrim_path failed: %w", err)
	}
	if err := syncManagedLLMTrimDatabaseConfigIfOwned(g.currentConfig().UpstreamBaseURL, pathValue); err != nil {
		return fmt.Errorf("同步受管 llmtrim 统计数据库配置失败: %w", err)
	}
	if err := executeLLMTrimCommandWithTimeout(ctx, pathValue, 45*time.Second, "setup"); err != nil {
		return err
	}
	if !waitForLLMTrimStateContext(ctx, pathValue, true) {
		return fmt.Errorf("llmtrim did not reach running state within context (port=%s)", llmtrimDaemonAddress)
	}
	if installDir, dirErr := llmtrimInstallDirectory(); dirErr == nil {
		if stateErr := writeManagedToolState(installDir, managedToolState{DesiredRunning: true, Running: true}); stateErr != nil {
			log.Printf("保存 llmtrim 启动状态失败: %v", stateErr)
		}
	}
	// 路由已确认切换到 llmtrim。监听保持运行，但已有 /v1 连接不能
	// 在两条上游链路间迁移，因此主动关闭并由客户端重新建立。
	g.closeGatewayProxyConnections()
	return nil
}

func (g *gateway) persistLLMTrimPath(pathValue string) error {
	if err := updateConfigValue(g.configPath, "llmtrim_path", pathValue); err != nil {
		return err
	}
	g.configMu.Lock()
	g.config.LLMTrimPath = pathValue
	g.configMu.Unlock()
	return nil
}

func validateLLMTrimPath(pathValue string) (string, error) {
	absolute, err := filepath.Abs(pathValue)
	if err != nil {
		return "", fmt.Errorf("invalid llmtrim path: %w", err)
	}
	info, err := os.Stat(absolute)
	if err != nil {
		return "", fmt.Errorf("llmtrim executable unavailable: %w", err)
	}
	if info.IsDir() || !strings.EqualFold(filepath.Base(absolute), "llmtrim.exe") {
		return "", errors.New("path must point to llmtrim.exe")
	}
	return absolute, nil
}

func llmtrimExecutableExists(pathValue string) bool {
	if strings.TrimSpace(pathValue) == "" || !strings.EqualFold(filepath.Base(pathValue), llmtrimExecutableName) {
		return false
	}
	info, err := os.Stat(pathValue)
	return err == nil && !info.IsDir()
}

func executeLLMTrimCommand(ctx context.Context, executable string, args ...string) error {
	return executeLLMTrimCommandWithTimeout(ctx, executable, 15*time.Second, args...)
}

func executeLLMTrimCommandWithTimeout(ctx context.Context, executable string, timeout time.Duration, args ...string) error {
	commandContext, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	command := exec.CommandContext(commandContext, executable, args...)
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	// 不使用 CombinedOutput：setup/stop 拉起或操作后台 daemon 时，子进程可能
	// 继承 stdout/stderr 管道，导致 Wait 一直等到 daemon 退出才返回。
	stdout, err := os.CreateTemp("", "code-manager-llmtrim-stdout-*.log")
	if err != nil {
		return fmt.Errorf("create llmtrim stdout log: %w", err)
	}
	stdoutPath := stdout.Name()
	defer os.Remove(stdoutPath)
	defer stdout.Close()
	stderr, err := os.CreateTemp("", "code-manager-llmtrim-stderr-*.log")
	if err != nil {
		return fmt.Errorf("create llmtrim stderr log: %w", err)
	}
	stderrPath := stderr.Name()
	defer os.Remove(stderrPath)
	defer stderr.Close()
	command.Stdout = stdout
	command.Stderr = stderr
	if err := command.Run(); err != nil {
		if errors.Is(commandContext.Err(), context.DeadlineExceeded) {
			return fmt.Errorf("llmtrim %s timed out", strings.Join(args, " "))
		}
		message := readCommandOutput(stderrPath)
		if message == "" {
			message = readCommandOutput(stdoutPath)
		}
		if message == "" {
			message = err.Error()
		}
		return errors.New(message)
	}
	return nil
}

func readCommandOutput(filePath string) string {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

func queryLLMTrimRunning(executable string) (bool, int, string, error) {
	if strings.TrimSpace(executable) == "" {
		return false, 0, "尚未接管 llmtrim.exe", nil
	}
	processID := findProcessIDByExecutable(executable)
	portOpen := isTCPPortOpen(llmtrimDaemonAddress)
	if processID == 0 {
		if portOpen {
			return false, 0, "43117 端口已被其他进程占用，未确认当前 llmtrim.exe 已启动", nil
		}
		return false, 0, "llmtrim daemon 未监听 " + llmtrimDaemonAddress, nil
	}
	if !portOpen {
		return false, processID, "llmtrim.exe 进程正在运行，但尚未监听 " + llmtrimDaemonAddress, nil
	}
	return true, processID, "llmtrim daemon 已由当前配置路径的进程监听 " + llmtrimDaemonAddress, nil
}

func isTCPPortOpen(address string) bool {
	connection, err := net.DialTimeout("tcp", address, 300*time.Millisecond)
	if err != nil {
		return false
	}
	_ = connection.Close()
	return true
}

func waitForLLMTrimState(executable string, wantRunning bool, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		running, _, _, err := queryLLMTrimRunning(executable)
		if err == nil && running == wantRunning {
			return true
		}
		time.Sleep(200 * time.Millisecond)
	}
	return false
}

func waitForLLMTrimStateContext(ctx context.Context, executable string, wantRunning bool) bool {
	ticker := time.NewTicker(200 * time.Millisecond)
	defer ticker.Stop()
	for {
		running, _, _, err := queryLLMTrimRunning(executable)
		if err == nil && running == wantRunning {
			return true
		}
		select {
		case <-ctx.Done():
			return false
		case <-ticker.C:
		}
	}
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(value); err != nil {
		log.Printf("write JSON response failed: %v", err)
	}
}

type configScalarUpdate struct {
	field string
	value string
	tag   string
	style yaml.Style
}

func updateConfigValue(configPath, field, value string) error {
	return updateConfigScalars(configPath, []configScalarUpdate{{field: field, value: value, tag: "!!str", style: yaml.DoubleQuotedStyle}})
}

func updateConfigBool(configPath, field string, value bool) error {
	return updateConfigScalars(configPath, []configScalarUpdate{{field: field, value: strconv.FormatBool(value), tag: "!!bool"}})
}

func updateStartupConfig(configPath string, startupEnabled, backgroundStart bool) error {
	return updateConfigScalars(configPath, []configScalarUpdate{
		{field: "startup_enabled", value: strconv.FormatBool(startupEnabled), tag: "!!bool"},
		{field: "background_start", value: strconv.FormatBool(backgroundStart), tag: "!!bool"},
	})
}

func updateConfigScalars(configPath string, updates []configScalarUpdate) error {
	if len(updates) == 0 {
		return nil
	}
	pending := make(map[string]configScalarUpdate, len(updates))
	for _, update := range updates {
		if update.field == "" {
			return errors.New("config field must not be empty")
		}
		pending[update.field] = update
	}
	data, err := os.ReadFile(configPath)
	if err != nil {
		return fmt.Errorf("read config: %w", err)
	}
	var document yaml.Node
	if err := yaml.Unmarshal(data, &document); err != nil {
		return fmt.Errorf("parse config: %w", err)
	}
	if len(document.Content) != 1 || document.Content[0].Kind != yaml.MappingNode {
		return errors.New("config root must be a YAML mapping")
	}
	mapping := document.Content[0]
	for index := 0; index+1 < len(mapping.Content); index += 2 {
		field := mapping.Content[index].Value
		update, found := pending[field]
		if !found {
			continue
		}
		valueNode := mapping.Content[index+1]
		valueNode.Kind = yaml.ScalarNode
		valueNode.Tag = update.tag
		valueNode.Value = update.value
		valueNode.Style = update.style
		delete(pending, field)
	}
	for _, update := range pending {
		keyNode := &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: update.field}
		valueNode := &yaml.Node{Kind: yaml.ScalarNode, Tag: update.tag, Value: update.value, Style: update.style}
		mapping.Content = append(mapping.Content, keyNode, valueNode)
	}
	updated, err := yaml.Marshal(&document)
	if err != nil {
		return fmt.Errorf("encode config: %w", err)
	}
	info, err := os.Stat(configPath)
	if err != nil {
		return fmt.Errorf("stat config: %w", err)
	}
	temporary, err := os.CreateTemp(filepath.Dir(configPath), ".code-manager-config-*")
	if err != nil {
		return fmt.Errorf("create temporary config: %w", err)
	}
	temporaryPath := temporary.Name()
	defer func() { _ = os.Remove(temporaryPath) }()
	if err := temporary.Chmod(info.Mode().Perm()); err != nil {
		_ = temporary.Close()
		return fmt.Errorf("set temporary config permissions: %w", err)
	}
	if _, err := temporary.Write(updated); err != nil {
		_ = temporary.Close()
		return fmt.Errorf("write temporary config: %w", err)
	}
	if err := temporary.Close(); err != nil {
		return fmt.Errorf("close temporary config: %w", err)
	}
	if err := os.Rename(temporaryPath, configPath); err != nil {
		return fmt.Errorf("replace config: %w", err)
	}
	return nil
}

func (g *gateway) health(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_, _ = io.WriteString(w, `{"status":"ok"}`)
}

func (g *gateway) forward(w http.ResponseWriter, r *http.Request) {
	finishLocalHTTP1Request := g.beginLocalHTTP1Request()
	defer finishLocalHTTP1Request()
	g.registerProxyConnection(r.Context())
	config := g.currentConfig()
	if config.LocalAPIKey != "" && r.Header.Get("Authorization") != "Bearer "+config.LocalAPIKey {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	if isWebSocketUpgradeAttempt(r) {
		key, err := validateWebSocketUpgrade(r)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		g.forwardWebSocket(w, r, config, key, finishLocalHTTP1Request)
		return
	}
	if r.Method != http.MethodGet && r.Method != http.MethodHead && r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	proxyContext, leaveProxyRequest, entered := g.enterProxyRequest(r.Context())
	if !entered {
		state, message := g.proxyRequestUnavailable()
		status := http.StatusServiceUnavailable
		if state == proxyStateUnavailable {
			status = http.StatusBadGateway
		}
		writeJSON(w, status, proxyStatusResponse{
			Running:       false,
			State:         state,
			ListenAddress: config.ListenAddress,
			Message:       message,
			Connections:   g.connectionStatusSnapshot(),
		})
		return
	}
	defer leaveProxyRequest()
	requestID := fmt.Sprintf("gateway-%d", atomic.AddUint64(&llmtrimRequestSequence, 1))
	requestStarted := time.Now()
	log.Printf("request %s: received %s %s", requestID, r.Method, r.URL.Path)
	llmtrimRunning := g.observeLLMTrimRunning(config.LLMTrimPath)
	if !llmtrimRunning {
		// This controls only whether the direct upstream transport starts WS
		// carrier negotiation. Local WS support and the llmtrim route remain on.
		proxyContext = withUpstreamWebSocketEnabled(proxyContext, config.UpstreamWebSocketEnabled)
	}
	client, err := g.selectUpstreamClient(llmtrimRunning)
	if err != nil {
		log.Printf("request %s: llmtrim proxy client unavailable for %s: %v", requestID, r.URL.Path, err)
		http.Error(w, "llmtrim proxy unavailable", http.StatusBadGateway)
		return
	}
	if r.Body != nil {
		stopRequestBody := context.AfterFunc(proxyContext, func() { _ = r.Body.Close() })
		defer func() { _ = stopRequestBody() }()
	}
	retryConfig := makeRetrySettings(config)
	canRetry := retryEnabled(retryConfig)
	var requestBody io.Reader
	var replay *replayBody
	contentLength := int64(-1)
	if r.Method == http.MethodPost {
		if r.ContentLength > maxRequestBytes {
			http.Error(w, "request body exceeds 64 MiB", http.StatusRequestEntityTooLarge)
			return
		}
		if r.Body != nil {
			r.Body = http.MaxBytesReader(w, r.Body, maxRequestBytes)
		}
		bodyForRetry := io.Reader(r.Body)
		contentLength = r.ContentLength
		if canRetry {
			cache := g.retryBodyCache
			if cache == nil {
				cache = newRetryBodyCache(filepath.Dir(g.configPath))
			}
			var err error
			if bodyForRetry == nil {
				bodyForRetry = bytes.NewReader(nil)
			}
			replay, err = cache.capture(bodyForRetry)
			if err != nil {
				var maxBytesError *http.MaxBytesError
				if errors.As(err, &maxBytesError) {
					http.Error(w, "request body exceeds 64 MiB", http.StatusRequestEntityTooLarge)
					return
				}
				log.Printf("capture retry body failed for %s: %v", r.URL.Path, err)
				http.Error(w, "读取请求体失败", http.StatusBadRequest)
				return
			}
			defer replay.Close()
			contentLength = replay.size
		} else {
			requestBody = bodyForRetry
		}
	}
	target, err := joinUpstreamURL(config.UpstreamBaseURL, strings.TrimPrefix(r.URL.Path, "/v1/"), r.URL.RawQuery)
	if err != nil {
		http.Error(w, "invalid upstream target", http.StatusBadGateway)
		return
	}
	requestHeaders := make(http.Header)
	copyHeaders(requestHeaders, r.Header)
	requestHeaders.Set("Authorization", "Bearer "+config.UpstreamAPIKey)
	route := "baseline upstream client"
	if llmtrimRunning {
		route = "llmtrim HTTP proxy"
	}
	log.Printf("request %s: forwarding upstream through %s after %s", requestID, route, time.Since(requestStarted).Round(time.Millisecond))
	response, err := roundTripWithRetry(proxyContext, client, r.Method, target, requestHeaders, contentLength, requestBody, replay, retryConfig)
	if err != nil {
		if proxyContext.Err() != nil {
			return
		}
		var maxBytesError *http.MaxBytesError
		if errors.As(err, &maxBytesError) {
			http.Error(w, "request body exceeds 64 MiB", http.StatusRequestEntityTooLarge)
			return
		}
		log.Printf("upstream request failed for %s: %v", r.URL.Path, err)
		http.Error(w, "upstream request failed", http.StatusBadGateway)
		return
	}
	// 已选定最终响应后不会再重试，流式响应期间也不应继续占用请求体缓存或临时文件。
	if replay != nil {
		replay.Close()
	}
	defer response.Body.Close()
	copyHeaders(w.Header(), response.Header)
	w.WriteHeader(response.StatusCode)
	if _, err := io.Copy(w, response.Body); err != nil {
		log.Printf("response stream failed for %s: %v", r.URL.Path, err)
	}
}

func joinUpstreamURL(base, suffix, rawQuery string) (string, error) {
	u, err := url.Parse(base)
	if err != nil {
		return "", err
	}
	suffix = strings.TrimPrefix(suffix, "/")
	if suffix != "" {
		switch {
		case u.Path == "":
			u.Path = "/" + suffix
		case strings.HasSuffix(u.Path, "/"):
			u.Path += suffix
		default:
			u.Path += "/" + suffix
		}
	}
	if rawQuery != "" {
		if u.RawQuery == "" {
			u.RawQuery = rawQuery
		} else {
			u.RawQuery += "&" + rawQuery
		}
	}
	return u.String(), nil
}

func copyHeaders(destination, source http.Header) {
	for key, values := range source {
		if isHopByHopHeader(key) || strings.EqualFold(key, "Authorization") || strings.EqualFold(key, "Host") {
			continue
		}
		for _, value := range values {
			destination.Add(key, value)
		}
	}
}

func isHopByHopHeader(key string) bool {
	switch strings.ToLower(key) {
	case "connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade":
		return true
	default:
		return false
	}
}

func requestLogger(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		started := time.Now()
		next.ServeHTTP(w, r)
		// 只记录真实的 OpenAI 兼容代理请求，忽略管理页面和状态轮询。
		if !strings.HasPrefix(r.URL.Path, "/v1/") {
			return
		}
		log.Printf("%s %s completed in %s", r.Method, r.URL.Path, time.Since(started).Round(time.Millisecond))
	})
}

func redactURL(raw string) string {
	u, err := url.Parse(raw)
	if err != nil {
		return "configured"
	}
	u.User = nil
	return u.String()
}
