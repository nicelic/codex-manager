<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from 'vue'

const statusPollInterval = 1000
const managementWebSocketConnectTimeout = 2500
const llmtrimCommandTimeout = 50000
let statusPollTimer = 0
let commandStatusPollTimer = 0
let managementSocketConnectTimer = 0
let managementSocket = null
let managementWebSocketActive = false
let statusPollRunning = false
let healthRequestRunning = false
let proxyStatusRequest = null
let proxyOperationToken = 0
let proxyStartController = null
let logStatusRequestRunning = false
let llmtrimLogStatusRequestRunning = false
let llmtrimStatusRequestRunning = false
let rtkStatusRequestRunning = false
let snipStatusRequestRunning = false
let gortexStatusRequestRunning = false
let pendingLLMTrimTarget = null
let llmtrimOperationToken = 0
let confirmationAction = null
let confirmationTrigger = null
let applicationUpdatePollTimer = 0

const online = ref(false)
const applicationVersion = ref('')
const applicationIsDevMode = ref(false)
const applicationUpdateChecked = ref(false)
const applicationHasUpdate = ref(false)
const applicationLatestVersion = ref('')
const applicationShowHistory = ref(false)
const applicationReleases = reactive({ items: [], page: 0, loading: false, loaded: false, hasMore: true })
const selectedApplicationVersion = ref('')
const applicationUpdateWorking = ref(false)
const applicationUpdateRestarting = ref(false)
const applicationUpdateNotice = ref('')
const checking = ref(false)
const loadingSettings = ref(true)
const checkedAt = ref('尚未检查')
const activeAddress = window.location.host
const activeTab = ref('rtk')
const managementProtocol = ref('http1.1')
const apiKey = ref('')
const proxy = reactive({
  running: false,
  state: 'stopped',
  processId: 0,
  listenAddress: '',
  loading: true,
  working: false,
  connections: {
    localHTTP1: 0,
    localWebSocket: 0,
    upstreamH2: 0,
    upstreamH2WebSocket: false,
    upstreamH3: 0,
    upstreamH3WebSocket: false,
    upstreamWebSocket: 0,
    upstreamStreams: 0,
  },
})
const proxyNotice = ref('')
const proxyActive = computed(() => Boolean(proxy.running || proxy.state === 'connecting' || proxy.working))
const logViewer = reactive({ showing: false, loading: true, working: false })
const logNotice = ref('')
const llmtrimLogViewer = reactive({ showing: false, loading: true, working: false })
const llmtrimLogNotice = ref('')
const llmtrim = reactive({ path: '', version: '', running: false, desiredRunning: false, activationState: 'not_installed', installed: false, configured: false, directoryExists: false, stateDirExists: false, trayRunning: false, trayProcessId: 0, residual: false, statusMessage: '', loading: true, working: false, commandInFlight: false })
const llmtrimNotice = ref('')
const llmtrimProcess = reactive({ id: 0, port: '' })
const llmtrimReleases = reactive({ items: [], page: 0, loading: false, loaded: false, hasMore: true })
const selectedLLMTrimVersion = ref('')
const llmtrimInstallWorking = ref(false)
const llmtrimDeleteWorking = ref(false)
const rtk = reactive({ path: '', installed: false, directoryExists: false, version: '', userPath: false, systemPath: false, codexAvailable: false, codexConfigured: false, codexResidual: false, claudeAvailable: false, claudeHookConfigured: false, claudePromptConfigured: false, claudeConfigured: false, claudeResidual: false, copilotAvailable: false, copilotConfigured: false, cursorAvailable: false, cursorConfigured: false, running: false, desiredRunning: false, activationState: 'not_installed', blockedBy: '', trustNotice: '', modifiedAgents: [], loading: true, working: false })
const rtkNotice = ref('')
const rtkReleases = reactive({ items: [], page: 0, loading: false, loaded: false, hasMore: true })
const selectedRTKVersion = ref('')
const rtkInstallWorking = ref(false)
const rtkDeleteWorking = ref(false)
const snip = reactive({ path: '', installed: false, directoryExists: false, version: '', userPath: false, systemPath: false, running: false, desiredRunning: false, cleanupRequired: false, activationState: 'not_installed', blockedBy: '', trustNotice: '', trustStatus: 'not_applicable', trustCommand: '', trustRequired: false, trustSteps: [], trustShellOpen: false, modifiedAgents: [], agents: [], loading: true, working: false })
const snipNotice = ref('')
const snipReleases = reactive({ items: [], page: 0, loading: false, loaded: false, hasMore: true })
const selectedSnipVersion = ref('')
const snipInstallWorking = ref(false)
const snipDeleteWorking = ref(false)
const snipTrustWorking = ref(false)
const gortex = reactive({ path: '', managedInstalled: false, managedRootExists: false, version: '', installing: false, installed: false, running: false, anyProcessRunning: false, unmanagedProcessRunning: false, integrationPresent: false, processId: 0, activationState: 'not_installed', codexAvailable: false, codexConfigured: false, codexComplete: false, claudeAvailable: false, claudeConfigured: false, claudeComplete: false, cursorAvailable: false, cursorConfigured: false, cursorComplete: false, copilotAvailable: false, copilotConfigured: false, copilotComplete: false, openCodeAvailable: false, openCodeConfigured: false, openCodeComplete: false, antigravityAvailable: false, antigravityConfigured: false, antigravityComplete: false, geminiAvailable: false, geminiConfigured: false, geminiComplete: false, codexPrompt: false, codexPromptComplete: false, claudePrompt: false, claudePromptComplete: false, cursorPrompt: false, cursorPromptComplete: false, copilotPrompt: false, copilotPromptComplete: false, openCodePrompt: false, openCodePromptComplete: false, antigravityPrompt: false, antigravityPromptComplete: false, geminiPrompt: false, geminiPromptComplete: false, codexHook: false, codexHookComplete: false, claudeHook: false, claudeHookComplete: false, copilotHook: false, copilotHookComplete: false, openCodeHook: false, openCodeHookComplete: false, antigravityHook: false, antigravityHookComplete: false, geminiHook: false, geminiHookComplete: false, userPath: false, systemPath: false, codexTrustStatus: 'not_applicable', codexTrustRequired: false, codexTrustNotice: '', codexTrustSteps: [], trackedProjects: [], projectMCPEnabled: false, projectMCPProjects: [], defaultProject: '', statusKnown: false, loading: true, working: false, installWorking: false, notice: '' })
const gortexDiagnostics = reactive({ loading: false, doctorOk: false, doctorOutput: '', doctorError: '', statusOk: false, statusOutput: '', statusError: '' })
const gortexProjectPath = ref('')
const gortexReleases = reactive({ items: [], page: 0, loading: false, loaded: false, hasMore: true })
const selectedGortexVersion = ref('')
const gortexBusy = computed(() => gortex.working || gortex.installWorking || gortex.installing)
function gortexVersionKey(value) {
  const text = String(value || '').trim().toLowerCase()
  const match = text.match(/v?\d+(?:\.\d+){2}/)
  return match ? match[0].replace(/^v/, '') : text
}
function gortexReleaseIsCurrent(tagName) {
  const selectedKey = gortexVersionKey(tagName)
  const currentKey = gortexVersionKey(gortex.version)
  return Boolean(gortex.managedInstalled && selectedKey && currentKey && selectedKey === currentKey)
}
const gortexSelectedVersionIsCurrent = computed(() => gortexReleaseIsCurrent(selectedGortexVersion.value))
const gortexVersionActionBlocked = computed(() => gortexBusy.value || gortex.running)
const gortexUninstallBlocked = computed(() => gortex.loading || !gortex.statusKnown || gortexBusy.value || gortex.running)
const upstreamH2Label = computed(() => `h2${proxy.connections.upstreamH2WebSocket ? '(ws)' : ''}_${proxy.connections.upstreamH2}`)
const upstreamH3Label = computed(() => `h3${proxy.connections.upstreamH3WebSocket ? '(ws)' : ''}_${proxy.connections.upstreamH3}`)
const rtkBlocked = computed(() => !rtk.running && (snip.running || snip.desiredRunning || rtk.blockedBy === 'snip'))
const snipBlocked = computed(() => !snip.running && (rtk.running || rtk.desiredRunning || snip.blockedBy === 'rtk'))
const llmtrimCanControl = computed(() => llmtrim.running || llmtrim.trayRunning || llmtrim.installed)
const startup = reactive({ enabled: false, background: false, working: false, exiting: false })
const startupNotice = ref('')
const confirmation = reactive({ open: false, title: '', description: '', confirmLabel: '确定' })
const confirmationCancelButton = ref(null)
const confirmationConfirmButton = ref(null)
const confirmationExecuting = ref(false)
const exitState = ref('idle')
const exitWarnings = ref([])
const pageInteractionLocked = computed(() => exitState.value !== 'idle' || applicationUpdateRestarting.value)
const exitTitle = computed(() => {
  if (exitState.value === 'stopping') return '正在停止并退出'
  if (exitState.value === 'completed') return '已停止并退出'
  if (exitState.value === 'warning') return '已退出，但部分清理未完成'
  return '退出连接已断开'
})
const exitDescription = computed(() => {
  if (exitState.value === 'stopping') return '正在安全停止代理、llmtrim、RTK、snip 和日志窗口。'
  if (exitState.value === 'completed') return '所有受管服务和页面操作均已停止。此提示会保留在当前页面中央。'
  if (exitState.value === 'warning') return 'code-Manager 将退出；请根据下方信息查看清理记录。'
  return '为避免在退出过程中继续写入配置，本页面将保持锁定。'
})
const settings = reactive({
  listenAddress: '',
  upstreamBaseURL: '',
  upstreamWebSocketEnabled: true,
})
const retry = reactive({
  enabled: false,
  count: '',
  intervalSeconds: '',
  statusCodes: '',
})
const retrySaving = reactive({
  enabled: false,
  count: false,
  intervalSeconds: false,
  statusCodes: false,
})
const retryEditVersion = { enabled: 0, count: 0, intervalSeconds: 0, statusCodes: 0 }
const retrySaveVersion = { enabled: 0, count: 0, intervalSeconds: 0, statusCodes: 0 }
const retrySaveQueue = { enabled: Promise.resolve(), count: Promise.resolve(), intervalSeconds: Promise.resolve(), statusCodes: Promise.resolve() }
const connectionSettingRestartWorking = ref(false)
const saving = reactive({
  listenAddress: false,
  upstreamBaseURL: false,
  upstreamAPIKey: false,
  upstreamWebSocketEnabled: false,
})
const notices = reactive({
  listenAddress: '',
  upstreamBaseURL: '',
  upstreamAPIKey: '',
  upstreamWebSocketEnabled: '',
})

function formatModifiedAgents(names) {
  const labels = { codex: 'Codex', 'claude-code': 'Claude Code', cursor: 'Cursor', copilot: 'GitHub Copilot' }
  return names.map((name) => labels[name] || name).join('、')
}

function activationLabel(tool, activeLabel = '已安装/运行中') {
  if (tool.loading) return '读取中'
  if (tool.activationState === 'attention') return '需要处理'
  if (tool.activationState === 'conflict') return '冲突'
  if (tool.running) return activeLabel
  if (tool.installed) return '已安装/已停止'
  return '未安装'
}

function gortexArtifactLabel(present, complete) {
  if (!present) return '未写入'
  return complete ? '已写入' : '配置残缺/已修改'
}

function agentStatusLabel(agent) {
  const labels = { codex: 'Codex', 'claude-code': 'Claude Code', cursor: 'Cursor', copilot: 'GitHub Copilot' }
  const name = labels[agent.name] || agent.name
  if (!agent.hook_exists) return `${name} Hook 文件不存在，未修改`
  if (agent.repair_needed) return `${name} Hook 已偏离受管路径，需修复`
  if (agent.modified) {
    if (agent.name === 'codex' && agent.trust === 'trusted') return `${name} Hook 已由本程序接入，已信任`
    if (agent.name === 'codex' && agent.trust === 'disabled') return `${name} Hook 已由本程序接入，但功能已关闭`
    if (agent.name === 'codex' && (agent.trust === 'untrusted' || agent.trust === 'unknown')) return `${name} Hook 已由本程序接入，待信任`
    return `${name} Hook 已由本程序接入`
  }
  if (agent.configured && agent.name === 'codex' && agent.trust === 'trusted') return `${name} Hook 已配置（非本程序受管），已信任`
  if (agent.configured && agent.name === 'codex' && agent.trust === 'disabled') return `${name} Hook 已配置（非本程序受管），但功能已关闭`
  if (agent.configured && agent.name === 'codex' && (agent.trust === 'untrusted' || agent.trust === 'unknown')) return `${name} Hook 已配置（非本程序受管），待信任`
  if (agent.configured) return `${name} Hook 已配置（非本程序受管）`
  return `${name} Hook 未配置`
}

function rtkClaudeStatusLabel() {
  if (!rtk.claudeAvailable) return 'Claude Code 未检测到'
  const hook = rtk.claudeHookConfigured ? 'Hook 已配置' : 'Hook 未配置'
  const prompt = rtk.claudeResidual ? 'CLAUDE.md 规则待处理' : (rtk.claudePromptConfigured ? 'CLAUDE.md 规则已集成' : 'CLAUDE.md 规则未集成')
  return `Claude Code ${hook}，${prompt}`
}

async function checkHealth({ silent = false } = {}) {
  if (healthRequestRunning) return
  healthRequestRunning = true
  if (!silent) checking.value = true
  try {
    const response = await fetch('/healthz', { cache: 'no-store' })
    online.value = response.ok
  } catch {
    online.value = false
  } finally {
    checkedAt.value = new Date().toLocaleTimeString()
    if (!silent) checking.value = false
    healthRequestRunning = false
  }
}

async function loadApplicationVersion() {
  try {
    const response = await fetch('/api/application/identity', { cache: 'no-store' })
    if (!response.ok) return
    const data = await response.json()
    applicationVersion.value = typeof data.version === 'string' ? data.version.trim() : ''
    applicationIsDevMode.value = Boolean(data.is_dev_mode)
  } catch {
    applicationVersion.value = ''
    applicationIsDevMode.value = false
  }
}

async function loadApplicationReleases({ more = false, silent = false } = {}) {
  if (applicationReleases.loading || applicationUpdateRestarting.value) return
  const page = more ? applicationReleases.page + 1 : 1
  applicationReleases.loading = true
  if (!silent) applicationUpdateNotice.value = ''
  try {
    const response = await fetch(`/api/application/releases?page=${page}`, { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '版本接口返回了无效响应')
    }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 code-Manager 版本失败')
    const incoming = Array.isArray(data.releases) ? data.releases : []
    applicationReleases.items = more ? [...applicationReleases.items, ...incoming] : incoming
    applicationReleases.page = Number(data.page) || page
    applicationReleases.hasMore = Boolean(data.has_more)
    applicationReleases.loaded = true
    applicationUpdateChecked.value = true
    if (typeof data.has_update === 'boolean') {
      applicationHasUpdate.value = data.has_update
    }
    if (typeof data.latest_version === 'string' && data.latest_version.trim()) {
      applicationLatestVersion.value = data.latest_version.trim()
    }
    if (typeof data.is_dev_mode === 'boolean') {
      applicationIsDevMode.value = data.is_dev_mode
    }
    if (!applicationReleases.items.some((item) => item.tag_name === selectedApplicationVersion.value && item.available)) {
      const firstAvailable = applicationReleases.items.find((item) => item.available)
      selectedApplicationVersion.value = firstAvailable ? firstAvailable.tag_name : ''
    }
  } catch (error) {
    applicationUpdateNotice.value = error.message || '读取 code-Manager 版本失败'
  } finally {
    applicationReleases.loading = false
  }
}

function handleApplicationVersionChange() {
  if (selectedApplicationVersion.value === '__load_more__') {
    selectedApplicationVersion.value = ''
    loadApplicationReleases({ more: true })
  }
}

function requestApplicationUpdateLatest(event) {
  const target = applicationLatestVersion.value || (applicationReleases.items.find((item) => item.available)?.tag_name)
  if (!target) return
  selectedApplicationVersion.value = target
  requestApplicationUpdate(event)
}

function requestApplicationUpdate(event) {
  if (applicationUpdateWorking.value || applicationUpdateRestarting.value || !selectedApplicationVersion.value || selectedApplicationVersion.value === '__load_more__') return
  if (applicationIsDevMode.value) {
    applicationUpdateNotice.value = '当前处于开发源码目录，已锁定更新，避免覆盖开发中的产物与源码。'
    return
  }
  const isCurrent = selectedApplicationVersion.value === applicationVersion.value
  openConfirmation({
    title: isCurrent ? `重新安装 ${selectedApplicationVersion.value}？` : `安装 ${selectedApplicationVersion.value}？`,
    description: `将在更新前自动停止顶部代理及运行中的 RTK、snip、llmtrim，并停止 Gortex daemon 且清理相关进程以释放文件锁，然后下载并安全替换 code-Manager.exe。失败时会自动恢复当前 EXE。`,
    confirmLabel: isCurrent ? '重新安装' : '开始安装',
    onConfirm: installApplicationUpdate,
    trigger: event?.currentTarget,
  })
}

async function installApplicationUpdate() {
  if (applicationUpdateWorking.value || !selectedApplicationVersion.value) return
  const targetVersion = selectedApplicationVersion.value
  const currentVersion = applicationVersion.value
  applicationUpdateWorking.value = true
  applicationUpdateNotice.value = ''
  try {
    if (proxy.running || proxy.state === 'connecting') {
      applicationUpdateNotice.value = '正在停止顶部代理…'
      await controlProxy('stop')
    }
    if (rtk.running || rtk.desiredRunning) {
      applicationUpdateNotice.value = '正在停止 RTK…'
      await controlRTK()
    }
    if (snip.running || snip.desiredRunning || snip.cleanupRequired) {
      applicationUpdateNotice.value = '正在停止 snip…'
      await controlSnip()
    }
    if (llmtrim.running || llmtrim.trayRunning) {
      applicationUpdateNotice.value = '正在停止 llmtrim…'
      await controlLLMTrim('stop')
    }
    if (gortex.running || gortex.anyProcessRunning) {
      applicationUpdateNotice.value = '正在停止 Gortex daemon…'
      await controlGortex('stop')
    }

    applicationUpdateNotice.value = '正在请求下载并准备更新…'
    const response = await fetch('/api/application/update', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ tag_name: targetVersion }),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '更新接口返回了无效响应')
    }
    if (!response.ok || !data.accepted) throw new Error(data.message || responseText.trim() || '启动更新失败')
    applicationUpdateNotice.value = data.message || '正在替换并重启 code-Manager。'
    applicationUpdateRestarting.value = true
    waitForApplicationUpdate(data.target_version || targetVersion, currentVersion)
  } catch (error) {
    applicationUpdateNotice.value = error.message || '启动更新失败'
  } finally {
    if (!applicationUpdateRestarting.value) applicationUpdateWorking.value = false
  }
}

function waitForApplicationUpdate(targetVersion, currentVersion) {
  window.clearTimeout(applicationUpdatePollTimer)
  let attempts = 0
  let disconnected = false
  const poll = async () => {
    attempts += 1
    try {
      const response = await fetch('/api/application/identity', { cache: 'no-store' })
      if (!response.ok) throw new Error('新版服务尚未就绪')
      const data = await response.json()
      const version = typeof data.version === 'string' ? data.version.trim() : ''
      if (version === targetVersion && (disconnected || targetVersion !== currentVersion)) {
        window.location.reload()
        return
      }
      if (disconnected && version === currentVersion && targetVersion !== currentVersion) {
        applicationUpdateRestarting.value = false
        applicationUpdateWorking.value = false
        applicationUpdateNotice.value = `更新未完成，已恢复 ${currentVersion}。`
        scheduleStatusPoll(0)
        return
      }
    } catch {
      disconnected = true
    }
    if (attempts >= 120) {
      applicationUpdateRestarting.value = false
      applicationUpdateWorking.value = false
      applicationUpdateNotice.value = '等待新版启动超时；请检查 code-Manager 是否已恢复运行。'
      scheduleStatusPoll(0)
      return
    }
    applicationUpdatePollTimer = window.setTimeout(poll, 1000)
  }
  poll()
}

function applyProxyStatus(data, { silent = false } = {}) {
	proxy.running = Boolean(data.running)
	proxy.state = data.state || (proxy.running ? 'running' : 'stopped')
	proxy.processId = Number(data.process_id) || 0
	proxy.listenAddress = data.listen_address || ''
	const connections = data.connections || {}
	proxy.connections.localHTTP1 = Math.max(0, Number(connections.local_http1) || 0)
	proxy.connections.localWebSocket = Math.max(0, Number(connections.local_ws) || 0)
	proxy.connections.upstreamH2 = Math.max(0, Number(connections.upstream_h2) || 0)
	proxy.connections.upstreamH2WebSocket = Boolean(connections.upstream_h2_ws)
	proxy.connections.upstreamH3 = Math.max(0, Number(connections.upstream_h3) || 0)
	proxy.connections.upstreamH3WebSocket = Boolean(connections.upstream_h3_ws)
	proxy.connections.upstreamWebSocket = Math.max(0, Number(connections.upstream_ws) || 0)
	proxy.connections.upstreamStreams = Math.max(0, Number(connections.upstream_streams) || 0)
	proxy.loading = false
	if (!silent && data.message) proxyNotice.value = data.message
}

function applyLogStatus(viewer, notice, data, { silent = false } = {}) {
  viewer.showing = Boolean(data.showing)
	viewer.loading = false
  if (!silent && data.message) notice.value = data.message
}

function applyLLMTrimStatus(data, { silent = false } = {}) {
  llmtrim.path = data.path || ''
  llmtrim.version = data.version || ''
  llmtrim.running = Boolean(data.running)
  llmtrim.desiredRunning = Boolean(data.desired_running)
  llmtrim.activationState = data.activation_state || 'not_installed'
  llmtrim.installed = Boolean(data.installed)
  llmtrim.configured = Boolean(data.configured)
  llmtrim.directoryExists = Boolean(data.directory_exists)
  llmtrim.stateDirExists = Boolean(data.state_dir_exists)
  llmtrim.trayRunning = Boolean(data.tray_running)
  llmtrim.trayProcessId = Number(data.tray_process_id) || 0
  llmtrim.residual = Boolean(data.residual)
  llmtrim.statusMessage = data.message || ''
  llmtrimProcess.id = Number(data.process_id) || 0
  llmtrimProcess.port = data.port || ''
  llmtrim.loading = false
  const targetReached = pendingLLMTrimTarget !== null && (pendingLLMTrimTarget ? llmtrim.running : (!llmtrim.running && !llmtrim.trayRunning))
  if (targetReached) {
    // 真实状态优先于控制命令响应，目标状态一旦达到就立即结束处理中。
    pendingLLMTrimTarget = null
    llmtrim.working = false
    llmtrim.commandInFlight = false
    llmtrimNotice.value = llmtrim.running ? 'llmtrim 已启动，后台状态已同步。' : 'llmtrim 和 llmtrim-tray 已停止，相关环境变量已清理。'
  }
  if (!silent && data.message && !data.running) llmtrimNotice.value = data.message
}

function applyRTKStatus(data, { silent = false } = {}) {
  rtk.path = data.path || ''
  rtk.installed = Boolean(data.installed)
  rtk.directoryExists = Boolean(data.directory_exists)
  rtk.version = data.version || ''
  rtk.userPath = Boolean(data.user_path)
  rtk.systemPath = Boolean(data.system_path)
  rtk.codexAvailable = Boolean(data.codex_available)
  rtk.codexConfigured = Boolean(data.codex_configured)
  rtk.codexResidual = Boolean(data.codex_residual)
  rtk.claudeAvailable = Boolean(data.claude_available)
  rtk.claudeHookConfigured = Boolean(data.claude_hook_configured)
  rtk.claudePromptConfigured = Boolean(data.claude_prompt_configured)
  rtk.claudeConfigured = Boolean(data.claude_configured)
  rtk.claudeResidual = Boolean(data.claude_residual)
  rtk.copilotAvailable = Boolean(data.copilot_available)
  rtk.copilotConfigured = Boolean(data.copilot_configured)
  rtk.cursorAvailable = Boolean(data.cursor_available)
  rtk.cursorConfigured = Boolean(data.cursor_configured)
  rtk.running = Boolean(data.running)
  rtk.desiredRunning = Boolean(data.desired_running)
  rtk.activationState = data.activation_state || 'not_installed'
	rtk.blockedBy = data.blocked_by || ''
	rtk.trustNotice = data.trust_notice || ''
	rtk.modifiedAgents = Array.isArray(data.modified_agents) ? data.modified_agents : []
	rtk.loading = false
	if (!silent && data.message) rtkNotice.value = data.message
}

function applySnipStatus(data, { silent = false } = {}) {
  snip.path = data.path || ''
  snip.installed = Boolean(data.installed)
  snip.directoryExists = Boolean(data.directory_exists)
  snip.version = data.version || ''
  snip.userPath = Boolean(data.user_path)
  snip.systemPath = Boolean(data.system_path)
  snip.running = Boolean(data.running)
  snip.desiredRunning = Boolean(data.desired_running)
	snip.cleanupRequired = Boolean(data.cleanup_required)
  snip.activationState = data.activation_state || 'not_installed'
  snip.blockedBy = data.blocked_by || ''
  snip.trustNotice = data.trust_notice || ''
  snip.trustStatus = data.trust_status || 'not_applicable'
  snip.trustCommand = data.trust_command || ''
  snip.trustRequired = Boolean(data.trust_required)
  snip.trustSteps = Array.isArray(data.trust_steps) ? data.trust_steps : []
	snip.trustShellOpen = Boolean(data.trust_shell_open)
	snip.modifiedAgents = Array.isArray(data.modified_agents) ? data.modified_agents : []
	snip.agents = Array.isArray(data.agents) ? data.agents : []
	snip.loading = false
	if (!silent && data.message) snipNotice.value = data.message
}

function applyGortexStatus(data, { silent = false } = {}) {
  gortex.statusKnown = Boolean(data && Object.prototype.hasOwnProperty.call(data, 'installing') && Object.prototype.hasOwnProperty.call(data, 'managed_installed') && Object.prototype.hasOwnProperty.call(data, 'running') && Object.prototype.hasOwnProperty.call(data, 'any_process_running') && Object.prototype.hasOwnProperty.call(data, 'integration_present'))
  gortex.path = data.path || ''
  gortex.managedInstalled = Boolean(data.managed_installed)
  gortex.managedRootExists = Boolean(data.managed_root_exists)
  gortex.version = data.version || ''
  gortex.installed = Boolean(data.installed)
  gortex.installing = Boolean(data.installing)
  gortex.running = Boolean(data.running)
  gortex.anyProcessRunning = Boolean(data.any_process_running)
  gortex.unmanagedProcessRunning = Boolean(data.unmanaged_process_running)
  gortex.integrationPresent = Boolean(data.integration_present)
  gortex.processId = Number(data.process_id) || 0
  gortex.activationState = data.activation_state || 'not_installed'
  gortex.codexAvailable = Boolean(data.codex_available)
  gortex.codexConfigured = Boolean(data.codex_configured)
  gortex.codexComplete = Boolean(data.codex_complete)
  gortex.claudeAvailable = Boolean(data.claude_available)
  gortex.claudeConfigured = Boolean(data.claude_configured)
  gortex.claudeComplete = Boolean(data.claude_complete)
  gortex.cursorAvailable = Boolean(data.cursor_available)
  gortex.cursorConfigured = Boolean(data.cursor_configured)
  gortex.cursorComplete = Boolean(data.cursor_complete)
  gortex.copilotAvailable = Boolean(data.copilot_available)
  gortex.copilotConfigured = Boolean(data.copilot_configured)
  gortex.copilotComplete = Boolean(data.copilot_complete)
  gortex.openCodeAvailable = Boolean(data.opencode_available)
  gortex.openCodeConfigured = Boolean(data.opencode_configured)
  gortex.openCodeComplete = Boolean(data.opencode_complete)
  gortex.antigravityAvailable = Boolean(data.antigravity_available)
  gortex.antigravityConfigured = Boolean(data.antigravity_configured)
  gortex.antigravityComplete = Boolean(data.antigravity_complete)
  gortex.geminiAvailable = Boolean(data.gemini_available)
  gortex.geminiConfigured = Boolean(data.gemini_configured)
  gortex.geminiComplete = Boolean(data.gemini_complete)
  gortex.codexPrompt = Boolean(data.codex_prompt)
  gortex.codexPromptComplete = Object.prototype.hasOwnProperty.call(data, 'codex_prompt_complete') ? Boolean(data.codex_prompt_complete) : gortex.codexPrompt
  gortex.claudePrompt = Boolean(data.claude_prompt)
  gortex.claudePromptComplete = Object.prototype.hasOwnProperty.call(data, 'claude_prompt_complete') ? Boolean(data.claude_prompt_complete) : gortex.claudePrompt
  gortex.cursorPrompt = Boolean(data.cursor_prompt)
  gortex.cursorPromptComplete = Object.prototype.hasOwnProperty.call(data, 'cursor_prompt_complete') ? Boolean(data.cursor_prompt_complete) : gortex.cursorPrompt
  gortex.copilotPrompt = Boolean(data.copilot_prompt)
  gortex.copilotPromptComplete = Object.prototype.hasOwnProperty.call(data, 'copilot_prompt_complete') ? Boolean(data.copilot_prompt_complete) : gortex.copilotPrompt
  gortex.openCodePrompt = Boolean(data.opencode_prompt)
  gortex.openCodePromptComplete = Object.prototype.hasOwnProperty.call(data, 'opencode_prompt_complete') ? Boolean(data.opencode_prompt_complete) : gortex.openCodePrompt
  gortex.antigravityPrompt = Boolean(data.antigravity_prompt)
  gortex.antigravityPromptComplete = Object.prototype.hasOwnProperty.call(data, 'antigravity_prompt_complete') ? Boolean(data.antigravity_prompt_complete) : gortex.antigravityPrompt
  gortex.geminiPrompt = Boolean(data.gemini_prompt)
  gortex.geminiPromptComplete = Object.prototype.hasOwnProperty.call(data, 'gemini_prompt_complete') ? Boolean(data.gemini_prompt_complete) : gortex.geminiPrompt
  gortex.codexHook = Boolean(data.codex_hook)
  gortex.codexHookComplete = Object.prototype.hasOwnProperty.call(data, 'codex_hook_complete') ? Boolean(data.codex_hook_complete) : gortex.codexHook
  gortex.claudeHook = Boolean(data.claude_hook)
  gortex.claudeHookComplete = Object.prototype.hasOwnProperty.call(data, 'claude_hook_complete') ? Boolean(data.claude_hook_complete) : gortex.claudeHook
  gortex.copilotHook = Boolean(data.copilot_hook)
  gortex.copilotHookComplete = Object.prototype.hasOwnProperty.call(data, 'copilot_hook_complete') ? Boolean(data.copilot_hook_complete) : gortex.copilotHook
  gortex.openCodeHook = Boolean(data.opencode_hook)
  gortex.openCodeHookComplete = Object.prototype.hasOwnProperty.call(data, 'opencode_hook_complete') ? Boolean(data.opencode_hook_complete) : gortex.openCodeHook
  gortex.antigravityHook = Boolean(data.antigravity_hook)
  gortex.antigravityHookComplete = Object.prototype.hasOwnProperty.call(data, 'antigravity_hook_complete') ? Boolean(data.antigravity_hook_complete) : gortex.antigravityHook
  gortex.geminiHook = Boolean(data.gemini_hook)
  gortex.geminiHookComplete = Object.prototype.hasOwnProperty.call(data, 'gemini_hook_complete') ? Boolean(data.gemini_hook_complete) : gortex.geminiHook
  gortex.userPath = Boolean(data.user_path)
  gortex.systemPath = Boolean(data.system_path)
  gortex.codexTrustStatus = data.codex_trust_status || 'not_applicable'
  gortex.codexTrustRequired = Boolean(data.codex_trust_required)
  gortex.codexTrustNotice = data.codex_trust_notice || ''
  gortex.codexTrustSteps = Array.isArray(data.codex_trust_steps) ? data.codex_trust_steps : []
  gortex.trackedProjects = Array.isArray(data.tracked_projects) ? data.tracked_projects : []
  gortex.projectMCPEnabled = Boolean(data.project_mcp_enabled)
  gortex.projectMCPProjects = Array.isArray(data.project_mcp_projects) ? data.project_mcp_projects : []
  gortex.defaultProject = data.default_project || ''
  gortex.loading = false
  if (!silent && data.message) gortex.notice = data.message
}

function applyManagementStatusEvent(event) {
  if (!event || event.type !== 'status') return
  online.value = true
  checkedAt.value = new Date().toLocaleTimeString()
  if (event.proxy) applyProxyStatus(event.proxy, { silent: true })
  if (event.logs) applyLogStatus(logViewer, logNotice, event.logs, { silent: true })
  if (event.llmtrim_logs) applyLogStatus(llmtrimLogViewer, llmtrimLogNotice, event.llmtrim_logs, { silent: true })
  if (event.llmtrim) applyLLMTrimStatus(event.llmtrim, { silent: true })
  if (event.rtk) applyRTKStatus(event.rtk, { silent: true })
  if (event.snip) applySnipStatus(event.snip, { silent: true })
  if (event.gortex) applyGortexStatus(event.gortex, { silent: true })
}

function managementWebSocketURL() {
  const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${scheme}//${window.location.host}/api/events`
}

function stopManagementEvents() {
  window.clearTimeout(managementSocketConnectTimer)
  managementSocketConnectTimer = 0
  const socket = managementSocket
	managementSocket = null
	managementWebSocketActive = false
	managementProtocol.value = 'http1.1'
  if (socket && socket.readyState < WebSocket.CLOSING) socket.close()
}

function startManagementEvents() {
  if (managementSocket || managementWebSocketActive) return
	if (typeof WebSocket !== 'function') {
		managementProtocol.value = 'http1.1'
		scheduleStatusPoll(0)
    return
  }
  let socket
  try {
    socket = new WebSocket(managementWebSocketURL())
	} catch {
		managementProtocol.value = 'http1.1'
		scheduleStatusPoll(0)
    return
  }
  managementSocket = socket
  let opened = false
  managementSocketConnectTimer = window.setTimeout(() => {
    if (!opened && managementSocket === socket) socket.close()
  }, managementWebSocketConnectTimeout)
  socket.onopen = () => {
    if (managementSocket !== socket) return
		opened = true
		managementWebSocketActive = true
		managementProtocol.value = 'ws'
    window.clearTimeout(managementSocketConnectTimer)
    managementSocketConnectTimer = 0
    window.clearTimeout(statusPollTimer)
    statusPollTimer = 0
  }
  socket.onmessage = (message) => {
    if (managementSocket !== socket) return
    try {
      applyManagementStatusEvent(JSON.parse(message.data))
    } catch {
      // 单条事件无效时保留最近的有效状态，等待下一条快照。
    }
  }
  socket.onclose = () => {
    if (managementSocket !== socket) return
		managementSocket = null
		managementWebSocketActive = false
		managementProtocol.value = 'http1.1'
    window.clearTimeout(managementSocketConnectTimer)
    managementSocketConnectTimer = 0
    if (!pageInteractionLocked.value) scheduleStatusPoll(0)
  }
}

async function loadSettings() {
  loadingSettings.value = true
  try {
    const response = await fetch('/api/settings', { cache: 'no-store' })
    const contentType = response.headers.get('content-type') || ''
    if (!response.ok || !contentType.includes('application/json')) {
      throw new Error('当前运行的是旧版 code-Manager，请退出旧进程后重新启动新版 EXE。')
    }
    const data = await response.json()
    settings.listenAddress = data.listen_address
    settings.upstreamBaseURL = data.upstream_base_url
    settings.upstreamWebSocketEnabled = data.upstream_websocket_enabled !== false
    apiKey.value = data.upstream_api_key || ''
    startup.enabled = Boolean(data.startup_enabled)
    startup.background = startup.enabled && Boolean(data.background_start)
    retry.enabled = Boolean(data.retry_enabled)
    retry.count = data.retry_count ?? ''
    retry.intervalSeconds = data.retry_interval_seconds ?? ''
    retry.statusCodes = data.retry_status_codes ?? ''
  } catch (error) {
    notices.listenAddress = error.message || '读取配置失败'
  } finally {
    loadingSettings.value = false
  }
}

async function updateStartupSetting(setting, enabled) {
  if (startup.working || startup.exiting) return
  startup.working = true
  startupNotice.value = ''
  try {
    const response = await fetch(`/api/settings/${setting}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value: String(enabled) }),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '服务返回了无效响应，请重启新版 EXE。')
    }
    if (!response.ok) throw new Error(data.message || '更新启动设置失败')
    if (setting === 'startup_enabled') {
      startup.enabled = enabled
      if (!enabled) startup.background = false
    } else {
      startup.background = enabled
    }
    startupNotice.value = data.message || '启动设置已更新。'
  } catch (error) {
    startupNotice.value = error.message || '更新启动设置失败'
    await loadSettings()
  } finally {
    startup.working = false
  }
}

function openConfirmation({ title, description, confirmLabel = '确定', onConfirm, trigger }) {
  if (pageInteractionLocked.value || confirmation.open || confirmationExecuting.value || typeof onConfirm !== 'function') return
  confirmation.title = title
  confirmation.description = description
  confirmation.confirmLabel = confirmLabel
  confirmationAction = onConfirm
  confirmationTrigger = trigger || null
  confirmation.open = true
  nextTick(() => confirmationConfirmButton.value?.focus())
}

function closeConfirmation({ restoreFocus = true } = {}) {
  if (!confirmation.open) return
  const trigger = confirmationTrigger
  confirmation.open = false
  confirmationAction = null
  confirmationTrigger = null
  if (restoreFocus) nextTick(() => trigger?.focus())
}

function cancelConfirmation() {
  closeConfirmation()
}

async function confirmPendingAction() {
  if (!confirmation.open || confirmationExecuting.value) return
  const action = confirmationAction
  closeConfirmation({ restoreFocus: false })
  confirmationExecuting.value = true
  try {
    await action()
  } finally {
    confirmationExecuting.value = false
  }
}

function handleConfirmationKeydown(event) {
  if (event.key === 'Escape') {
    event.preventDefault()
    return
  }
  if (event.key !== 'Tab') return
  const first = confirmationCancelButton.value
  const last = confirmationConfirmButton.value
  if (!first || !last) return
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault()
    first.focus()
  }
}

function exitApplication(event) {
  openConfirmation({
    title: '停止并退出 code-Manager？',
    description: '将停止 llmtrim、RTK、snip、代理和日志窗口，并清理相关配置。',
    onConfirm: confirmApplicationExit,
    trigger: event?.currentTarget,
  })
}

async function confirmApplicationExit() {
  if (pageInteractionLocked.value) return
  startup.exiting = true
  startupNotice.value = ''
  exitWarnings.value = []
  exitState.value = 'stopping'
  stopPageActivityForExit()
  let rejected = false
  try {
    const response = await fetch('/api/application/exit', { method: 'POST' })
    const responseText = await response.text()
    if (!response.ok) {
      rejected = true
      throw new Error(responseText.trim() || '退出请求失败')
    }
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '退出接口未返回有效结果')
    }
    if (!data.completed) throw new Error(data.message || '退出接口未确认完成')
    exitWarnings.value = Array.isArray(data.warnings) ? data.warnings : []
    exitState.value = data.clean === false ? 'warning' : 'completed'
  } catch (error) {
    if (rejected) {
      exitState.value = 'idle'
      startup.exiting = false
      startupNotice.value = error.message || '无法退出 code-Manager。'
      scheduleStatusPoll()
      return
    }
    exitState.value = 'disconnected'
  }
}

function stopPageActivityForExit() {
  window.clearTimeout(statusPollTimer)
  window.clearTimeout(commandStatusPollTimer)
  statusPollTimer = 0
  commandStatusPollTimer = 0
  pendingLLMTrimTarget = null
  llmtrimOperationToken++
  proxyOperationToken++
  if (proxyStartController) {
    proxyStartController.abort()
    proxyStartController = null
  }
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
}

async function loadProxyStatus({ silent = false, refresh = false } = {}) {
  if (proxyStatusRequest) {
    const loaded = await proxyStatusRequest.catch(() => false)
    if (!refresh) return loaded
  }
  const request = (async () => {
    if (!silent) proxy.loading = true
    try {
      const response = await fetch('/api/proxy', { cache: 'no-store' })
      const contentType = response.headers.get('content-type') || ''
      if (!response.ok || !contentType.includes('application/json')) {
        throw new Error('当前运行的 code-Manager 不支持代理控制，请重启新版 EXE。')
      }
      const data = await response.json()
      applyProxyStatus(data, { silent })
      return true
    } catch (error) {
      proxyNotice.value = error.message || '读取代理状态失败'
      return false
    } finally {
      if (!silent) proxy.loading = false
    }
  })()
  proxyStatusRequest = request
  try {
    return await request
  } finally {
    if (proxyStatusRequest === request) proxyStatusRequest = null
  }
}

async function controlProxy(action) {
  if (proxy.working && action !== 'stop') return false
  if (action === 'start') {
    if (typeof document !== 'undefined' && document.activeElement && typeof document.activeElement.blur === 'function') {
      document.activeElement.blur()
    }
    await Promise.all(Object.values(retrySaveQueue).map((p) => Promise.resolve(p).catch(() => false)))
  }
  const operationToken = ++proxyOperationToken
  if (action === 'stop' && proxyStartController) {
    proxyStartController.abort()
    proxyStartController = null
  }
  proxy.working = true
  proxyNotice.value = action === 'start' ? '正在建立上游 H2/H3 TLS 连接；扩展 CONNECT 是否可用由上游 SETTINGS 与握手结果决定。' : ''
  let controller = null
  if (action === 'start') {
    proxy.state = 'connecting'
    controller = new AbortController()
    proxyStartController = controller
  }
  try {
    const response = await fetch(`/api/proxy/${action}`, { method: 'POST', signal: controller?.signal })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '服务返回了无效响应，请重启新版 EXE。')
    }
    if (!response.ok) throw new Error(data.message || '代理操作失败')
    if (operationToken !== proxyOperationToken) return false
    proxy.running = Boolean(data.running)
    proxy.state = data.state || (proxy.running ? 'running' : 'stopped')
    proxy.processId = Number(data.process_id) || 0
    proxy.listenAddress = data.listen_address || ''
    if (action === 'start' && (!proxy.running || proxy.state !== 'running')) {
      throw new Error(data.message || '代理未能启动')
    }
    if (action === 'stop' && (proxy.running || proxy.state !== 'stopped')) {
      throw new Error(data.message || '代理未能停止')
    }
    proxyNotice.value = data.message || '代理状态已更新。'
    return true
  } catch (error) {
    if (operationToken !== proxyOperationToken) return false
    if (error.name === 'AbortError' && action === 'start') return false
    proxyNotice.value = error.message || '无法连接到 code-Manager。'
    return false
  } finally {
    if (proxyStartController === controller) proxyStartController = null
    if (operationToken === proxyOperationToken) {
      proxy.working = false
      await loadProxyStatus({ silent: true })
    }
  }
}

async function loadLogStatus({ silent = false } = {}) {
  if (logStatusRequestRunning) return
  logStatusRequestRunning = true
  if (!silent) logViewer.loading = true
  try {
    const response = await fetch('/api/logs', { cache: 'no-store' })
    const contentType = response.headers.get('content-type') || ''
    if (!response.ok || !contentType.includes('application/json')) {
      throw new Error('当前运行的 code-Manager 不支持日志窗口，请重启新版 EXE。')
    }
    const data = await response.json()
    applyLogStatus(logViewer, logNotice, data, { silent })
  } catch (error) {
    logNotice.value = error.message || '读取日志窗口状态失败'
  } finally {
    if (!silent) logViewer.loading = false
    logStatusRequestRunning = false
  }
}

async function controlLogViewer(action) {
  if (logViewer.working) return
  logViewer.working = true
  logNotice.value = ''
  try {
    const response = await fetch(`/api/logs/${action}`, { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '服务返回了无效响应，请重启新版 EXE。')
    }
    if (!response.ok) throw new Error(data.message || '日志窗口操作失败')
    logViewer.showing = Boolean(data.showing)
    logNotice.value = data.message || '日志窗口状态已更新。'
  } catch (error) {
    logNotice.value = error.message || '无法操作日志窗口。'
  } finally {
    logViewer.working = false
    await loadLogStatus({ silent: true })
  }
}

async function loadLLMTrimLogStatus({ silent = false } = {}) {
  if (llmtrimLogStatusRequestRunning) return
  llmtrimLogStatusRequestRunning = true
  if (!silent) llmtrimLogViewer.loading = true
  try {
    const response = await fetch('/api/llmtrim/logs', { cache: 'no-store' })
    const contentType = response.headers.get('content-type') || ''
    if (!response.ok || !contentType.includes('application/json')) {
      throw new Error('当前运行的 code-Manager 不支持 llmtrim 日志窗口，请重启新版 EXE。')
    }
    const data = await response.json()
    applyLogStatus(llmtrimLogViewer, llmtrimLogNotice, data, { silent })
  } catch (error) {
    llmtrimLogNotice.value = error.message || '读取 llmtrim 日志窗口状态失败'
  } finally {
    if (!silent) llmtrimLogViewer.loading = false
    llmtrimLogStatusRequestRunning = false
  }
}

async function controlLLMTrimLogViewer(action) {
  if (llmtrimLogViewer.working) return
  llmtrimLogViewer.working = true
  llmtrimLogNotice.value = ''
  try {
    const response = await fetch(`/api/llmtrim/logs/${action}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '服务返回了无效响应，请重启新版 EXE。')
    }
    if (!response.ok) throw new Error(data.message || 'llmtrim 日志窗口操作失败')
    llmtrimLogViewer.showing = Boolean(data.showing)
    llmtrimLogNotice.value = data.message || 'llmtrim 日志窗口状态已更新。'
  } catch (error) {
    llmtrimLogNotice.value = error.message || '无法操作 llmtrim 日志窗口。'
  } finally {
    llmtrimLogViewer.working = false
    await loadLLMTrimLogStatus({ silent: true })
  }
}

async function loadLLMTrimStatus({ silent = false } = {}) {
  if (llmtrimStatusRequestRunning) return
  llmtrimStatusRequestRunning = true
  if (!silent) llmtrim.loading = true
  try {
    const response = await fetch('/api/llmtrim', { cache: 'no-store' })
    const contentType = response.headers.get('content-type') || ''
    if (!response.ok || !contentType.includes('application/json')) {
      throw new Error('当前运行的 code-Manager 不支持 llmtrim 控制，请重启新版 EXE。')
    }
    const data = await response.json()
    applyLLMTrimStatus(data, { silent })
  } catch (error) {
    llmtrimNotice.value = error.message || '读取 llmtrim 状态失败'
  } finally {
    if (!silent) llmtrim.loading = false
    llmtrimStatusRequestRunning = false
  }
}

async function loadRTKStatus({ silent = false } = {}) {
  if (rtkStatusRequestRunning) return
  rtkStatusRequestRunning = true
  if (!silent) rtk.loading = true
  try {
    const response = await fetch('/api/rtk', { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'RTK 状态接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 RTK 状态失败')
    applyRTKStatus(data, { silent })
  } catch (error) {
    rtkNotice.value = error.message || '读取 RTK 状态失败'
  } finally {
    if (!silent) rtk.loading = false
    rtkStatusRequestRunning = false
  }
}

async function loadSnipStatus({ silent = false } = {}) {
  if (snipStatusRequestRunning) return
  snipStatusRequestRunning = true
  if (!silent) snip.loading = true
  try {
    const response = await fetch('/api/snip', { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'snip 状态接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 snip 状态失败')
    applySnipStatus(data, { silent })
  } catch (error) {
    snipNotice.value = error.message || '读取 snip 状态失败'
  } finally {
    if (!silent) snip.loading = false
    snipStatusRequestRunning = false
  }
}

async function loadGortexStatus({ silent = false } = {}) {
  if (gortexStatusRequestRunning) return
  gortexStatusRequestRunning = true
  if (!silent) gortex.loading = true
  try {
    const response = await fetch('/api/gortex', { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 状态接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 Gortex 状态失败')
    applyGortexStatus(data, { silent })
    if (!gortex.statusKnown) {
      gortex.notice = '当前运行的 code-Manager 不支持完整的 Gortex 状态保护，请重启新版 EXE。'
    }
  } catch (error) {
    gortex.statusKnown = false
    gortex.notice = error.message || '读取 Gortex 状态失败'
  } finally {
    if (!silent) gortex.loading = false
    gortexStatusRequestRunning = false
  }
}

async function controlGortex(action) {
  if (gortexBusy.value || !gortex.managedInstalled) return
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch(`/api/gortex/${action}`, { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 操作接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || 'Gortex 操作失败')
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    gortex.notice = `${data.message || 'Gortex 状态已更新。'}${warningText}`
  } catch (error) {
    gortex.notice = error.message || 'Gortex 操作失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function installGortex() {
  if (gortexVersionActionBlocked.value || gortex.loading || !gortex.statusKnown || !selectedGortexVersion.value || gortexSelectedVersionIsCurrent.value) {
    if (!selectedGortexVersion.value) gortex.notice = '请先加载并选择一个可用的 Gortex 版本。'
    else if (gortexSelectedVersionIsCurrent.value) gortex.notice = '所选版本与当前受管版本一致，无需重复安装。'
    return
  }
  gortex.installWorking = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/install', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ tag_name: selectedGortexVersion.value }) })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 安装接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '安装 Gortex 失败')
    gortex.notice = data.message || 'Gortex Release ZIP 已安装。'
  } catch (error) {
    gortex.notice = error.message || '安装 Gortex 失败'
  } finally {
    gortex.installWorking = false
    await loadGortexStatus({ silent: true })
  }
}

function requestInstallGortex(event) {
  if (gortexVersionActionBlocked.value || gortex.loading || !gortex.statusKnown || gortexSelectedVersionIsCurrent.value) return
  const actionLabel = gortex.managedInstalled ? '升级' : '安装'
  openConfirmation({
    title: `下载并${actionLabel} Gortex Release ZIP？`,
    description: `将先强制停止所有路径下的 gortex.exe（包括外部启动的 daemon/MCP），确认全部退出后再从官方发布源下载 Gortex ${selectedGortexVersion.value || '所选版本'} 的 Windows x64 ZIP。只有新 exe 下载、校验并解压成功后才会替换当前文件；不会执行 gortex install，也不会自动启动 daemon。`,
    confirmLabel: `开始${actionLabel}`,
    onConfirm: installGortex,
    trigger: event?.currentTarget,
  })
}

async function loadGortexReleases({ more = false } = {}) {
  if (gortexReleases.loading || gortexVersionActionBlocked.value) return
  const page = more ? gortexReleases.page + 1 : 1
  gortexReleases.loading = true
  gortex.notice = ''
  try {
    const response = await fetch(`/api/gortex/releases?page=${page}`, { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 版本接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 Gortex 版本失败')
    const incoming = Array.isArray(data.releases) ? data.releases : []
    gortexReleases.items = more ? [...gortexReleases.items, ...incoming] : incoming
    gortexReleases.page = Number(data.page) || page
    gortexReleases.hasMore = Boolean(data.has_more)
    gortexReleases.loaded = true
    if (!gortexReleases.items.some((item) => item.tag_name === selectedGortexVersion.value && item.available)) {
      const firstAvailable = gortexReleases.items.find((item) => item.available)
      selectedGortexVersion.value = firstAvailable ? firstAvailable.tag_name : ''
    }
  } catch (error) {
    gortex.notice = error.message || '读取 Gortex 版本失败'
  } finally {
    gortexReleases.loading = false
  }
}

function handleGortexVersionChange() {
  if (selectedGortexVersion.value === '__load_more__') {
    selectedGortexVersion.value = ''
    loadGortexReleases({ more: true })
  }
}

async function registerGortex() {
  if (gortexBusy.value || !gortex.managedInstalled) return
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/register', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 注册接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '注册 Gortex MCP 失败')
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    gortex.notice = `${data.message || 'Gortex MCP 已注册。'}${warningText}`
  } catch (error) {
    gortex.notice = error.message || '注册 Gortex MCP 失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function trustGortexCodex() {
  if (gortexBusy.value || !gortex.codexTrustRequired) return
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/trust', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Codex 信任接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '打开 Codex 信任窗口失败')
    gortex.notice = data.trust_notice || '已打开 Codex Hook 信任界面，请在 /hooks 中审核 Gortex。'
  } catch (error) {
    gortex.notice = error.message || '打开 Codex 信任窗口失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function runGortexDiagnostics() {
  if (gortexDiagnostics.loading || gortexBusy.value || !gortex.managedInstalled) return
  gortexDiagnostics.loading = true
  try {
    const response = await fetch('/api/gortex/diagnostics', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 诊断接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '运行 Gortex 诊断失败')
    gortexDiagnostics.doctorOk = Boolean(data.doctor_ok)
    gortexDiagnostics.doctorOutput = data.doctor_output || ''
    gortexDiagnostics.doctorError = data.doctor_error || ''
    gortexDiagnostics.statusOk = Boolean(data.status_ok)
    gortexDiagnostics.statusOutput = data.status_output || ''
    gortexDiagnostics.statusError = data.status_error || ''
    gortex.notice = data.message || 'Gortex 诊断已完成。'
  } catch (error) {
    gortex.notice = error.message || '运行 Gortex 诊断失败'
  } finally {
    gortexDiagnostics.loading = false
  }
}

async function removeGortex() {
  if (gortexBusy.value) return
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/remove', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 移除接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '移除 Gortex MCP 失败')
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    gortex.notice = `${data.message || 'Gortex MCP 已移除。'}${warningText}`
  } catch (error) {
    gortex.notice = error.message || '移除 Gortex MCP 失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function trackGortexProject() {
  const pathValue = gortexProjectPath.value.trim()
  gortexProjectPath.value = pathValue
  if (gortexBusy.value || !gortex.managedInstalled || !pathValue) {
    if (!pathValue) gortex.notice = '请输入要 track 的项目绝对路径。'
    return
  }
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/track', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: pathValue }) })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex track 接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || 'Gortex track 失败')
    gortex.notice = data.message || '项目已 track。'
    gortexProjectPath.value = ''
  } catch (error) {
    gortex.notice = error.message || 'Gortex track 失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function untrackGortexProject(pathValue) {
  if (gortexBusy.value || !gortex.managedInstalled || !pathValue) return
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/untrack', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: pathValue }) })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex untrack 接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || 'Gortex untrack 失败')
    gortex.notice = data.message || '项目已取消 track。'
  } catch (error) {
    gortex.notice = error.message || 'Gortex untrack 失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

function requestUninstallGortex(event) {
  if (gortexUninstallBlocked.value || (!gortex.managedInstalled && !gortex.managedRootExists && !gortex.trackedProjects.length && !gortex.projectMCPEnabled && !gortex.projectMCPProjects.length && !gortex.codexConfigured && !gortex.claudeConfigured && !gortex.cursorConfigured && !gortex.copilotConfigured && !gortex.openCodeConfigured && !gortex.antigravityConfigured && !gortex.geminiConfigured)) return
  openConfirmation({
    title: '确定卸载 Gortex 吗？',
    description: '将停止 daemon、取消已记录项目的 track、移除名为 gortex 的 MCP 配置，清理当前受管路径下仍在运行的 Gortex 进程，并删除 code-Manager.exe 同级受管 Gortex 目录。即使 Codex 的 gortex mcp 仍在运行，也会在卸载时结束它；其他 MCP、项目文件和用户目录中的非受管文件会保留。',
    onConfirm: uninstallGortex,
    trigger: event?.currentTarget,
  })
}

async function uninstallGortex() {
  if (gortexUninstallBlocked.value) return
  await loadGortexStatus({ silent: true })
  if (!gortex.statusKnown) {
    gortex.notice = '无法确认 Gortex 当前状态，已取消卸载；请刷新状态后重试。'
    return
  }
  if (gortex.installing || gortex.running) {
    gortex.notice = gortex.installing ? 'Gortex 正在安装，安装完成后才能卸载。' : 'Gortex daemon 正在运行，请先停止 daemon 后再卸载。'
    return
  }
  gortex.working = true
  gortex.notice = ''
  try {
    const response = await fetch('/api/gortex/uninstall', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'Gortex 卸载接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '卸载 Gortex 失败')
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    gortex.notice = `${data.message || 'Gortex 已卸载。'}${warningText}`
  } catch (error) {
    gortex.notice = error.message || '卸载 Gortex 失败'
  } finally {
    gortex.working = false
    await loadGortexStatus({ silent: true })
  }
}

async function loadRTKReleases({ more = false } = {}) {
  if (rtkReleases.loading) return
  const page = more ? rtkReleases.page + 1 : 1
  rtkReleases.loading = true
  rtkNotice.value = ''
  try {
    const response = await fetch(`/api/rtk/releases?page=${page}`, { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'RTK 版本接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 RTK 版本失败')
    const incoming = Array.isArray(data.releases) ? data.releases : []
    rtkReleases.items = more ? [...rtkReleases.items, ...incoming] : incoming
    rtkReleases.page = Number(data.page) || page
    rtkReleases.hasMore = Boolean(data.has_more)
    rtkReleases.loaded = true
    if (!rtkReleases.items.some((item) => item.tag_name === selectedRTKVersion.value && item.available)) {
      const firstAvailable = rtkReleases.items.find((item) => item.available)
      selectedRTKVersion.value = firstAvailable ? firstAvailable.tag_name : ''
    }
  } catch (error) {
    rtkNotice.value = error.message || '读取 RTK 版本失败'
  } finally {
    rtkReleases.loading = false
  }
}

function handleRTKVersionChange() {
  if (selectedRTKVersion.value === '__load_more__') {
    selectedRTKVersion.value = ''
    loadRTKReleases({ more: true })
  }
}

async function installRTK() {
  if (rtkInstallWorking.value || rtkBlocked.value || !selectedRTKVersion.value || selectedRTKVersion.value === '__load_more__') {
    if (!selectedRTKVersion.value) rtkNotice.value = '请先点击“加载”并选择一个可用版本。'
    return
  }
  rtkInstallWorking.value = true
  rtkNotice.value = ''
  try {
    const response = await fetch('/api/rtk/install', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ tag_name: selectedRTKVersion.value }) })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'RTK 安装接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '安装 RTK 失败')
    rtk.path = data.path || rtk.path
    rtk.installed = true
    rtk.directoryExists = true
    rtk.version = data.version || selectedRTKVersion.value
    rtk.userPath = Boolean(data.user_path)
    rtk.systemPath = Boolean(data.system_path)
    rtk.codexAvailable = Boolean(data.codex_available)
    rtk.codexConfigured = Boolean(data.codex_configured)
    rtk.claudeAvailable = Boolean(data.claude_available)
    rtk.claudeHookConfigured = Boolean(data.claude_hook_configured)
    rtk.claudePromptConfigured = Boolean(data.claude_prompt_configured)
    rtk.claudeConfigured = Boolean(data.claude_configured)
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    rtkNotice.value = `${data.message || `RTK ${rtk.version} 已安装。`}${warningText}`
  } catch (error) {
    rtkNotice.value = error.message || '安装 RTK 失败'
  } finally {
    rtkInstallWorking.value = false
    await loadRTKStatus({ silent: true })
  }
}

function requestUninstallRTK(event) {
  if (rtkDeleteWorking.value || rtkBlocked.value || (!rtk.installed && !rtk.directoryExists && !rtk.userPath && !rtk.systemPath && !rtk.codexConfigured && !rtk.codexResidual && !rtk.claudeConfigured && !rtk.claudeResidual && !rtk.copilotConfigured && !rtk.cursorConfigured)) return
  openConfirmation({
    title: '确定删除 RTK 吗？',
    description: '将清理 RTK-AI 目录、受管的用户和系统 PATH、AGENTS.md/CLAUDE.md 规则，以及 Claude/Copilot/Cursor 的受管 RTK Hook。未受管内容会保留。此操作不可撤销。',
    onConfirm: uninstallRTK,
    trigger: event?.currentTarget,
  })
}

async function uninstallRTK() {
  if (rtkDeleteWorking.value || rtkBlocked.value || (!rtk.installed && !rtk.directoryExists && !rtk.userPath && !rtk.systemPath && !rtk.codexConfigured && !rtk.codexResidual && !rtk.claudeConfigured && !rtk.claudeResidual && !rtk.copilotConfigured && !rtk.cursorConfigured)) return
  rtkDeleteWorking.value = true
  rtkNotice.value = ''
  try {
    const response = await fetch('/api/rtk/uninstall', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try { data = responseText ? JSON.parse(responseText) : {} } catch { throw new Error(responseText.trim() || 'RTK 删除接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '删除 RTK 失败')
    rtk.path = data.path || rtk.path
    rtk.installed = Boolean(data.installed)
    rtk.directoryExists = Boolean(data.directory_exists)
    rtk.userPath = Boolean(data.user_path)
    rtk.systemPath = Boolean(data.system_path)
    rtk.codexAvailable = Boolean(data.codex_available)
    rtk.codexConfigured = Boolean(data.codex_configured)
    rtk.codexResidual = Boolean(data.codex_residual)
    rtk.claudeAvailable = Boolean(data.claude_available)
    rtk.claudeHookConfigured = Boolean(data.claude_hook_configured)
    rtk.claudePromptConfigured = Boolean(data.claude_prompt_configured)
    rtk.claudeConfigured = Boolean(data.claude_configured)
    rtk.claudeResidual = Boolean(data.claude_residual)
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    rtkNotice.value = `${data.message || 'RTK 已删除。'}${warningText}`
  } catch (error) {
    rtkNotice.value = error.message || '删除 RTK 失败'
  } finally {
    rtkDeleteWorking.value = false
    await loadRTKStatus({ silent: true })
  }
}

async function controlRTK() {
  const stopping = rtk.running || rtk.desiredRunning
  if (rtk.working || (!stopping && rtkBlocked.value) || !rtk.installed) return
  rtk.working = true
  rtkNotice.value = ''
  const action = stopping ? 'stop' : 'start'
  try {
    const response = await fetch(`/api/rtk/${action}`, { method: 'POST' })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'RTK 操作接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || 'RTK 操作失败')
    rtkNotice.value = data.message || 'RTK 状态已更新。'
  } catch (error) {
    rtkNotice.value = error.message || 'RTK 操作失败'
  } finally {
    rtk.working = false
    await Promise.all([loadRTKStatus({ silent: true }), loadSnipStatus({ silent: true })])
  }
}

async function loadSnipReleases({ more = false } = {}) {
  if (snipReleases.loading) return
  const page = more ? snipReleases.page + 1 : 1
  snipReleases.loading = true
  try {
    const response = await fetch(`/api/snip/releases?page=${page}`, { cache: 'no-store' })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'snip 版本接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || '读取 snip 版本失败')
    const incoming = Array.isArray(data.releases) ? data.releases : []
    snipReleases.items = more ? [...snipReleases.items, ...incoming] : incoming
    snipReleases.page = Number(data.page) || page
    snipReleases.hasMore = Boolean(data.has_more)
    snipReleases.loaded = true
    if (!snipReleases.items.some((item) => item.tag_name === selectedSnipVersion.value && item.available)) {
      const first = snipReleases.items.find((item) => item.available)
      selectedSnipVersion.value = first ? first.tag_name : ''
    }
  } catch (error) {
    snipNotice.value = error.message || '读取 snip 版本失败'
  } finally {
    snipReleases.loading = false
  }
}

function handleSnipVersionChange() {
  if (selectedSnipVersion.value === '__load_more__') {
    selectedSnipVersion.value = ''
    loadSnipReleases({ more: true })
  }
}

async function installSnip() {
  if (snipInstallWorking.value || snipBlocked.value || snip.running || snip.desiredRunning || !selectedSnipVersion.value || selectedSnipVersion.value === '__load_more__') return
  snipInstallWorking.value = true
  snipNotice.value = ''
  try {
    const response = await fetch('/api/snip/install', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ tag_name: selectedSnipVersion.value }) })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'snip 安装接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || '安装 snip 失败')
    snipNotice.value = data.message || 'snip 已安装。'
  } catch (error) {
    snipNotice.value = error.message || '安装 snip 失败'
  } finally {
    snipInstallWorking.value = false
    await loadSnipStatus({ silent: true })
  }
}

async function controlSnip() {
  const stopping = snip.running || snip.desiredRunning || snip.cleanupRequired
  if (snip.working || (!stopping && snipBlocked.value) || !snip.installed) return
  snip.working = true
  snipNotice.value = ''
  const action = stopping ? 'stop' : 'start'
  try {
    const response = await fetch(`/api/snip/${action}`, { method: 'POST' })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'snip 操作接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || 'snip 操作失败')
    snipNotice.value = data.message || 'snip 状态已更新。'
  } catch (error) {
    snipNotice.value = error.message || 'snip 操作失败'
  } finally {
    snip.working = false
    await Promise.all([loadSnipStatus({ silent: true }), loadRTKStatus({ silent: true })])
  }
}

async function addSnipTrust() {
  if (snipTrustWorking.value || !snip.installed || snip.trustStatus === 'trusted') return
  snipTrustWorking.value = true
  snipNotice.value = ''
  try {
    const response = await fetch('/api/snip/trust', { method: 'POST' })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'Codex 信任接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || '打开 Codex 信任窗口失败')
    snip.trustStatus = data.trust_status || snip.trustStatus
    snip.trustNotice = data.trust_notice || snip.trustNotice
    snip.trustCommand = data.trust_command || snip.trustCommand
    snip.trustRequired = Boolean(data.trust_required)
    snip.trustSteps = Array.isArray(data.trust_steps) ? data.trust_steps : snip.trustSteps
    snip.trustShellOpen = Boolean(data.trust_shell_open)
    snipNotice.value = data.message || '已打开 PowerShell，请选择第 2 项，然后按键盘 Enter（回车）。'
  } catch (error) {
    snipNotice.value = error.message || '打开 Codex 信任窗口失败'
  } finally {
    snipTrustWorking.value = false
    await loadSnipStatus({ silent: true })
  }
}

function requestUninstallSnip(event) {
  if (snipDeleteWorking.value || snipBlocked.value || snip.running || snip.desiredRunning || (!snip.installed && !snip.directoryExists)) return
  openConfirmation({
    title: '确定删除 snip 吗？',
    description: '请先停止 snip；删除只会处理 Snip 目录及 code-Manager 写入的配置。',
    onConfirm: uninstallSnip,
    trigger: event?.currentTarget,
  })
}

async function uninstallSnip() {
  if (snipDeleteWorking.value || snipBlocked.value || snip.running || snip.desiredRunning || (!snip.installed && !snip.directoryExists)) return
  snipDeleteWorking.value = true
  snipNotice.value = ''
  try {
    const response = await fetch('/api/snip/uninstall', { method: 'POST' })
    const text = await response.text()
    let data = {}
    try { data = text ? JSON.parse(text) : {} } catch { throw new Error(text.trim() || 'snip 删除接口返回了无效响应') }
    if (!response.ok) throw new Error(data.message || text.trim() || '删除 snip 失败')
    snipNotice.value = data.message || 'snip 已删除。'
  } catch (error) {
    snipNotice.value = error.message || '删除 snip 失败'
  } finally {
    snipDeleteWorking.value = false
    await loadSnipStatus({ silent: true })
  }
}

async function loadLLMTrimReleases({ more = false } = {}) {
  if (llmtrimReleases.loading) return
  const page = more ? llmtrimReleases.page + 1 : 1
  llmtrimReleases.loading = true
  llmtrimNotice.value = ''
  try {
    const response = await fetch(`/api/llmtrim/releases?page=${page}`, { cache: 'no-store' })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '版本接口返回了无效响应')
    }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '读取 llmtrim 版本失败')
    const incoming = Array.isArray(data.releases) ? data.releases : []
    llmtrimReleases.items = more ? [...llmtrimReleases.items, ...incoming] : incoming
    llmtrimReleases.page = Number(data.page) || page
    llmtrimReleases.hasMore = Boolean(data.has_more)
    llmtrimReleases.loaded = true
    if (!llmtrimReleases.items.some((item) => item.tag_name === selectedLLMTrimVersion.value && item.available)) {
      const firstAvailable = llmtrimReleases.items.find((item) => item.available)
      selectedLLMTrimVersion.value = firstAvailable ? firstAvailable.tag_name : ''
    }
  } catch (error) {
    llmtrimNotice.value = error.message || '读取 llmtrim 版本失败'
  } finally {
    llmtrimReleases.loading = false
  }
}

function handleLLMTrimVersionChange() {
  if (selectedLLMTrimVersion.value === '__load_more__') {
    selectedLLMTrimVersion.value = ''
    loadLLMTrimReleases({ more: true })
  }
}

async function installLLMTrim() {
  if (llmtrimInstallWorking.value || !selectedLLMTrimVersion.value) {
    if (!selectedLLMTrimVersion.value) llmtrimNotice.value = '请先点击“加载”并选择一个可用版本。'
    return
  }
  llmtrimInstallWorking.value = true
  llmtrimNotice.value = ''
  try {
    const response = await fetch('/api/llmtrim/install', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ tag_name: selectedLLMTrimVersion.value }),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '安装接口返回了无效响应')
    }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '安装 llmtrim 失败')
    llmtrim.path = data.path || llmtrim.path
    llmtrim.version = data.version || llmtrim.version
    llmtrim.running = Boolean(data.running)
    llmtrimProcess.id = Number(data.process_id) || 0
    llmtrimProcess.port = data.port || llmtrimProcess.port
    llmtrimNotice.value = data.message || `llmtrim ${selectedLLMTrimVersion.value} 已安装并启动。`
    await loadLLMTrimStatus({ silent: true })
  } catch (error) {
    llmtrimNotice.value = error.message || '安装 llmtrim 失败'
  } finally {
    llmtrimInstallWorking.value = false
  }
}

function requestUninstallLLMTrim(event) {
  if (llmtrimDeleteWorking.value || llmtrimInstallWorking.value || !llmtrim.residual) return
  openConfirmation({
    title: '确定删除 llmtrim 吗？',
    description: '将清理 code-Manager 受管的 llmtrim 安装、安装目录中的统计数据库、历史共享的 tracking.db / tracking.db-wal / tracking.db-shm、环境、自启动、用户 CA、状态目录和 %USERPROFILE%\\.config\\llmtrim 配置目录。此操作不可撤销。',
    onConfirm: uninstallLLMTrim,
    trigger: event?.currentTarget,
  })
}

async function uninstallLLMTrim() {
  if (llmtrimDeleteWorking.value || llmtrimInstallWorking.value || !llmtrim.residual) return
  llmtrimDeleteWorking.value = true
  llmtrimNotice.value = ''
  try {
    const response = await fetch('/api/llmtrim/uninstall', { method: 'POST' })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '删除接口返回了无效响应')
    }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '删除 llmtrim 失败')
    llmtrim.path = data.path || ''
    llmtrim.running = Boolean(data.running)
    llmtrim.directoryExists = Boolean(data.directory_exists)
    llmtrim.stateDirExists = Boolean(data.state_dir_exists)
    llmtrim.trayRunning = Boolean(data.tray_running)
    llmtrim.trayProcessId = Number(data.tray_process_id) || 0
    llmtrim.residual = Boolean(data.residual)
    llmtrimProcess.id = Number(data.process_id) || 0
    llmtrimProcess.port = data.port || ''
    const warningText = Array.isArray(data.warnings) && data.warnings.length ? ` ${data.warnings.join(' ')}` : ''
    llmtrimNotice.value = `${data.message || 'llmtrim 已删除。'}${warningText}`
  } catch (error) {
    llmtrimNotice.value = error.message || '删除 llmtrim 失败'
  } finally {
    llmtrimDeleteWorking.value = false
    await loadLLMTrimStatus({ silent: true })
  }
}

async function controlLLMTrim(action) {
  llmtrimNotice.value = ''
  if (llmtrim.commandInFlight) return
  llmtrim.working = true
  llmtrim.commandInFlight = true
  pendingLLMTrimTarget = action === 'start'
  const operationToken = ++llmtrimOperationToken
  startCommandStatusPoll(operationToken)
  const controller = new AbortController()
  const timeout = window.setTimeout(() => controller.abort(), llmtrimCommandTimeout)
  try {
    const response = await fetch(`/api/llmtrim/${action}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
      signal: controller.signal,
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '服务返回了无效响应，请重启新版 EXE。')
    }
    if (!response.ok) throw new Error(data.message || 'llmtrim 操作失败')
    if (operationToken === llmtrimOperationToken && pendingLLMTrimTarget !== null) {
      await loadLLMTrimStatus({ silent: true })
      if (pendingLLMTrimTarget !== null) llmtrimNotice.value = data.message || '命令已执行，正在等待后台状态同步。'
    }
  } catch (error) {
    if (operationToken === llmtrimOperationToken && pendingLLMTrimTarget !== null) {
      llmtrimNotice.value = error.name === 'AbortError' ? '操作超时（50 秒）。页面会继续自动刷新状态，请查看后台进程和端口是否已启动。' : (error.message || '无法连接到 code-Manager。')
      pendingLLMTrimTarget = null
      llmtrim.working = false
    }
  } finally {
    window.clearTimeout(timeout)
    llmtrim.commandInFlight = false
  }
}

function llmtrimActionLabel() {
  if (llmtrim.working) return '处理中…'
  if (llmtrim.running || llmtrim.trayRunning) return '停止'
  return '启动'
}

function startCommandStatusPoll(operationToken) {
  if (pageInteractionLocked.value) return
  window.clearTimeout(commandStatusPollTimer)
  const poll = async () => {
    if (pageInteractionLocked.value || operationToken !== llmtrimOperationToken || pendingLLMTrimTarget === null) return
    await loadLLMTrimStatus({ silent: true })
    if (!pageInteractionLocked.value && operationToken === llmtrimOperationToken && pendingLLMTrimTarget !== null) {
      commandStatusPollTimer = window.setTimeout(poll, statusPollInterval)
    }
  }
  poll()
}

function scheduleStatusPoll(delay = statusPollInterval) {
  if (pageInteractionLocked.value || managementWebSocketActive) return
  window.clearTimeout(statusPollTimer)
  statusPollTimer = window.setTimeout(runStatusPoll, delay)
}

async function runStatusPoll() {
  if (pageInteractionLocked.value || managementWebSocketActive || document.visibilityState === 'hidden' || statusPollRunning) return
  statusPollRunning = true
  try {
    const tasks = [
      checkHealth({ silent: true }),
      loadProxyStatus({ silent: true }),
      loadLLMTrimStatus({ silent: true }),
      loadRTKStatus({ silent: true }),
      loadSnipStatus({ silent: true }),
      loadGortexStatus({ silent: true }),
    ]
    if (logViewer.showing || logViewer.working) tasks.push(loadLogStatus({ silent: true }))
    if (llmtrimLogViewer.showing || llmtrimLogViewer.working) tasks.push(loadLLMTrimLogStatus({ silent: true }))
    await Promise.all(tasks)
  } finally {
    statusPollRunning = false
    if (!pageInteractionLocked.value) scheduleStatusPoll()
  }
}

function removeLineBreaks(value) {
  return String(value ?? '').replace(/[\r\n]+/g, '')
}

function normalizeUpstreamBaseURL(value) {
  const normalized = removeLineBreaks(value).trim()
  if (!normalized) return ''

  let parsed
  try {
    parsed = new URL(normalized)
  } catch {
    return normalized
  }
  if (parsed.protocol !== 'https:' || !parsed.hostname) return normalized

  const queryOrHashIndex = normalized.search(/[?#]/)
  const authorityAndPath = queryOrHashIndex >= 0 ? normalized.slice(0, queryOrHashIndex) : normalized
  const suffix = queryOrHashIndex >= 0 ? normalized.slice(queryOrHashIndex) : ''
  const schemeIndex = authorityAndPath.indexOf('://')
  const pathStart = schemeIndex >= 0 ? authorityAndPath.indexOf('/', schemeIndex + 3) : -1
  if (pathStart < 0) return normalized
  return `${authorityAndPath.slice(0, pathStart)}${authorityAndPath.slice(pathStart).replace(/\/+$/, '')}${suffix}`
}

function setSettingValue(key, value) {
  if (key === 'upstreamAPIKey') {
    apiKey.value = value
    return
  }
  settings[key] = value
}

function sanitizeSettingInput(key, event) {
  const value = removeLineBreaks(event.target.value)
  if (event.target.value !== value) event.target.value = value
  setSettingValue(key, value)
}

function discardSettingEnter(key) {
  const value = key === 'upstreamAPIKey' ? apiKey.value : settings[key]
  setSettingValue(key, removeLineBreaks(value))
}

function sanitizeRetryInput(key, event) {
  if (proxyActive.value) return
  const value = String(event.target.value ?? '').replace(/\s+/g, '')
  if (event.target.value !== value) event.target.value = value
  retry[key] = value
  retryEditVersion[key]++
}

function normalizeRetryInputForBlur(key) {
  if (proxyActive.value) return
  const value = String(retry[key] ?? '').replace(/\s+/g, '')
  if ((key === 'count' || key === 'intervalSeconds') && value !== '' && !/^\d+$/.test(value)) {
    retry[key] = ''
    retryEditVersion[key]++
    return
  }
  retry[key] = value
  retryEditVersion[key]++
}

async function updateRetryEnabled(enabled) {
  if (proxyActive.value || connectionSettingRestartWorking.value || retrySaving.enabled || loadingSettings.value) return
  const previous = retry.enabled
  retry.enabled = enabled
  retryEditVersion.enabled++
  connectionSettingRestartWorking.value = true
  try {
    const saved = await saveRetrySetting('enabled', 'retry_enabled')
    if (saved && retry.enabled !== previous) await restartProxyForConnectionSetting('自动重试')
  } finally {
    connectionSettingRestartWorking.value = false
  }
}

function retrySettingValue(key) {
  if (key === 'enabled') return String(retry.enabled)
  return retry[key]
}

function saveRetrySetting(key, endpoint) {
  if (proxyActive.value && key !== 'enabled') return Promise.resolve(false)
  const version = ++retrySaveVersion[key]
  const editVersion = retryEditVersion[key]
  const value = retrySettingValue(key)
  retrySaveQueue[key] = retrySaveQueue[key]
    .catch(() => false)
    .then(async () => {
      retrySaving[key] = true
      try {
        const response = await fetch(`/api/settings/${endpoint}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ value }),
        })
        const responseText = await response.text()
        let data = {}
        try {
          data = responseText ? JSON.parse(responseText) : {}
        } catch {
          throw new Error(responseText.trim() || '保存失败')
        }
        if (!response.ok) throw new Error(data.message || '保存失败')
        if (version !== retrySaveVersion[key] || editVersion !== retryEditVersion[key]) return false
        if (key === 'enabled') {
          retry.enabled = data.value === 'true'
        } else {
          retry[key] = data.value ?? ''
        }
        return true
      } catch (error) {
        // 保留尚未失焦的新输入；下一次失焦会再次保存该字段。
        return false
      } finally {
        if (version === retrySaveVersion[key]) retrySaving[key] = false
      }
    })
  return retrySaveQueue[key]
}

function handleVisibilityChange() {
  if (pageInteractionLocked.value) return
  if (document.visibilityState === 'hidden') {
    window.clearTimeout(statusPollTimer)
    stopManagementEvents()
    return
  }
  startManagementEvents()
}

async function saveSetting(key, endpoint, value) {
  notices[key] = ''
  const normalizedValue = key === 'upstreamBaseURL' ? normalizeUpstreamBaseURL(value) : removeLineBreaks(value).trim()
  setSettingValue(key, normalizedValue)
  if (!normalizedValue) {
    notices[key] = '请输入有效值。'
    return
  }
  saving[key] = true
  try {
    const response = await fetch(`/api/settings/${endpoint}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value: normalizedValue }),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      if (!response.ok) throw new Error(responseText.trim() || '保存失败')
      throw new Error('服务返回了无效响应')
    }
    if (!response.ok) throw new Error(data.message || responseText.trim() || '保存失败')
    const savedValue = data.value ?? normalizedValue
    setSettingValue(key, savedValue)
    notices[key] = data.message
    if (key === 'upstreamAPIKey') {
      apiKey.value = savedValue
    }
  } catch (error) {
    notices[key] = error.message || '保存失败'
  } finally {
    saving[key] = false
  }
}

async function updateUpstreamWebSocketEnabled(enabled) {
  if (proxyActive.value || connectionSettingRestartWorking.value || saving.upstreamWebSocketEnabled || loadingSettings.value) return
  const previous = settings.upstreamWebSocketEnabled
  settings.upstreamWebSocketEnabled = enabled
  notices.upstreamWebSocketEnabled = ''
  saving.upstreamWebSocketEnabled = true
  connectionSettingRestartWorking.value = true
  try {
    const response = await fetch('/api/settings/upstream_websocket_enabled', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value: String(enabled) }),
    })
    const responseText = await response.text()
    let data = {}
    try {
      data = responseText ? JSON.parse(responseText) : {}
    } catch {
      throw new Error(responseText.trim() || '保存失败')
    }
    if (!response.ok) throw new Error(data.message || '保存失败')
    settings.upstreamWebSocketEnabled = data.value !== 'false'
    notices.upstreamWebSocketEnabled = data.message || '已保存。'
    if (settings.upstreamWebSocketEnabled !== previous) await restartProxyForConnectionSetting('上游 WS 承载')
  } catch (error) {
    settings.upstreamWebSocketEnabled = previous
    notices.upstreamWebSocketEnabled = error.message || '保存失败'
  } finally {
    saving.upstreamWebSocketEnabled = false
    connectionSettingRestartWorking.value = false
  }
}

function markConnectionSettingRestartFailure(settingLabel, fallback) {
  const detail = String(proxyNotice.value || '').trim()
  const reason = detail && !detail.startsWith('正在') && !detail.startsWith(`${settingLabel}配置已保存，正在`) ? detail : fallback
  proxyNotice.value = `${settingLabel}配置已保存，但自动重启代理失败：${reason}`
}

async function restartProxyForConnectionSetting(settingLabel) {
  const refreshed = await loadProxyStatus({ silent: true, refresh: true })
  if (!refreshed) {
    markConnectionSettingRestartFailure(settingLabel, '无法读取代理状态。')
    return false
  }
  if (proxy.state !== 'running' && proxy.state !== 'connecting') return true

  proxyNotice.value = `${settingLabel}配置已保存，正在停止代理以重建上游连接。`
  if (!await controlProxy('stop')) {
    markConnectionSettingRestartFailure(settingLabel, '停止代理失败。')
    return false
  }

  proxyNotice.value = `${settingLabel}配置已保存，正在按新配置建立上游连接。`
  if (!await controlProxy('start')) {
    markConnectionSettingRestartFailure(settingLabel, '启动代理失败。')
    return false
  }

  proxyNotice.value = `${settingLabel}配置已保存，代理已按新配置重新启动。`
  return true
}

onMounted(() => {
  checkHealth()
  loadApplicationVersion()
  loadSettings()
  document.addEventListener('visibilitychange', handleVisibilityChange)
  startManagementEvents()
})

onBeforeUnmount(() => {
  stopManagementEvents()
  window.clearTimeout(statusPollTimer)
  window.clearTimeout(commandStatusPollTimer)
  window.clearTimeout(applicationUpdatePollTimer)
  document.removeEventListener('visibilitychange', handleVisibilityChange)
})
</script>

<template>
  <main class="page-shell" :class="{ 'is-interaction-locked': pageInteractionLocked || confirmation.open }" :inert="pageInteractionLocked || confirmation.open ? '' : null" :aria-hidden="pageInteractionLocked || confirmation.open ? 'true' : null">
    <section class="panel">
      <div class="brand-row">
        <div class="logo">CM</div>
        <div>
          <p class="eyebrow">LOCAL GATEWAY</p>
          <h1 class="application-title">
            <span>code-Manager</span>
            <span v-if="applicationVersion" class="application-version">{{ applicationVersion }}</span>
          </h1>
        </div>
      </div>

      <p class="description">本地 API 网关提供管理页面；代理转发可由下方状态卡片启动或停止。</p>

      <div class="status-card" :class="{ online: proxy.running, connecting: proxy.state === 'connecting', unavailable: proxy.state === 'unavailable' }">
        <span class="status-dot" />
        <div>
          <strong>{{ proxy.loading ? '正在读取代理状态' : (proxy.state === 'connecting' ? '正在连接上游' : (proxy.state === 'unavailable' ? '上游不可用' : (proxy.running ? '代理运行中' : '代理已停止'))) }}</strong>
          <span>http://{{ proxy.listenAddress || activeAddress }}</span>
          <span>已完成 H2/H3 预热，扩展 CONNECT 由上游 SETTINGS 与握手结果决定</span>
          <span v-if="proxy.processId">进程 PID {{ proxy.processId }}</span>
        </div>
      </div>
      <p v-if="proxyNotice" class="notice proxy-notice">{{ proxyNotice }}</p>

      <div class="proxy-controls">
        <button type="button" class="proxy-button" :class="{ stop: proxy.running || proxy.state === 'connecting' }" :disabled="proxy.loading || (proxy.working && proxy.state !== 'connecting')" @click="controlProxy((proxy.running || proxy.state === 'connecting') ? 'stop' : 'start')">
          {{ proxy.state === 'connecting' ? '停止代理' : (proxy.working ? '处理中…' : (proxy.running ? '停止代理' : '启动代理')) }}
        </button>
        <button type="button" :class="{ stop: logViewer.showing }" :disabled="logViewer.loading || logViewer.working" @click="controlLogViewer(logViewer.showing ? 'hide' : 'show')">
          {{ logViewer.working ? '处理中…' : (logViewer.showing ? '关闭日志' : '显示日志') }}
        </button>
        <button type="button" :disabled="checking" @click="checkHealth">
          {{ checking ? '检查中…' : '刷新状态' }}
        </button>
      </div>
      <p v-if="logNotice" class="notice log-notice">{{ logNotice }}</p>

      <div class="info-grid">
        <div><span>HTTP/WS API 地址</span><code>http://{{ proxy.listenAddress || activeAddress }}/v1</code></div>
        <div class="application-update-card">
          <div class="application-update-header">
            <div class="application-update-meta">
              <span class="update-title">版本更新</span>
              <span class="current-version-tag">当前 {{ applicationVersion || '读取中' }}</span>
              <span v-if="applicationIsDevMode" class="update-badge dev">开发源码模式</span>
              <span v-else-if="applicationUpdateChecked && applicationHasUpdate" class="update-badge has-update">
                发现新版 {{ applicationLatestVersion }}
              </span>
              <span v-else-if="applicationUpdateChecked && !applicationHasUpdate" class="update-badge latest">
                已是最新版本
              </span>
            </div>
            <div class="application-update-actions">
              <button
                v-if="!applicationIsDevMode && applicationUpdateChecked && applicationHasUpdate"
                type="button"
                class="update-primary-button"
                :disabled="applicationUpdateWorking || applicationUpdateRestarting || applicationReleases.loading"
                @click="requestApplicationUpdateLatest"
              >
                {{ applicationUpdateWorking ? '升级中…' : `升级到 ${applicationLatestVersion}` }}
              </button>
              <button
                type="button"
                class="update-check-button"
                :class="{ 'is-loading': applicationReleases.loading }"
                :disabled="applicationUpdateWorking || applicationUpdateRestarting || applicationReleases.loading"
                @click="loadApplicationReleases()"
              >
                {{ applicationReleases.loading ? '检查中…' : (applicationUpdateChecked ? '重新检查' : '检查更新') }}
              </button>
              <button
                type="button"
                class="update-toggle-history-button"
                :disabled="applicationUpdateWorking || applicationUpdateRestarting"
                @click="applicationShowHistory = !applicationShowHistory"
              >
                {{ applicationShowHistory ? '收起版本列表' : '历史版本 / 回退' }}
              </button>
            </div>
          </div>

          <div v-if="applicationShowHistory" class="application-history-panel">
            <div class="application-history-controls">
              <span class="history-label">目标版本：</span>
              <select
                aria-label="选择 code-Manager 远端版本"
                v-model="selectedApplicationVersion"
                :disabled="applicationUpdateWorking || applicationUpdateRestarting || applicationReleases.loading"
                @change="handleApplicationVersionChange"
              >
                <option value="">{{ applicationReleases.loaded ? '请选择目标版本' : '请先点击检查更新' }}</option>
                <option
                  v-for="release in applicationReleases.items"
                  :key="release.tag_name"
                  :value="release.tag_name"
                  :disabled="!release.available"
                >
                  {{ release.tag_name }}{{ release.prerelease ? '（预发布）' : '' }}{{ !release.available ? `（${release.unavailable_reason || '不可安装'}）` : '' }}
                </option>
                <option v-if="applicationReleases.hasMore && applicationReleases.loaded" value="__load_more__">加载更多历史版本…</option>
              </select>
              <button
                type="button"
                class="install-button"
                :disabled="applicationIsDevMode || applicationUpdateWorking || applicationUpdateRestarting || applicationReleases.loading || !selectedApplicationVersion || selectedApplicationVersion === '__load_more__'"
                @click="requestApplicationUpdate"
              >
                {{ applicationUpdateWorking ? '安装中…' : (selectedApplicationVersion === applicationVersion ? '重新安装' : '安装所选') }}
              </button>
            </div>
            <p v-if="applicationIsDevMode" class="dev-mode-tip">开发源码模式下禁止自升级覆盖，避免破坏开发中文件与二进制。</p>
          </div>
        </div>
        <p v-if="applicationUpdateNotice" class="notice application-update-notice">{{ applicationUpdateNotice }}</p>
        <div><span>基线连接</span><code>H2/H3 多流 · 每连接最多 500 个活动流</code></div>
        <div class="connection-status"><span>连接状态</span><code><span>网页管理：{{ managementProtocol }}</span><span>本地监听：http1.1_{{ proxy.connections.localHTTP1 }}&nbsp;&nbsp;ws_{{ proxy.connections.localWebSocket }}</span><span>上游：{{ upstreamH2Label }}&nbsp;&nbsp;{{ upstreamH3Label }}&nbsp;&nbsp;ws_{{ proxy.connections.upstreamWebSocket }}&nbsp;&nbsp;流_{{ proxy.connections.upstreamStreams }}</span></code></div>
        <div><span>健康检查</span><code>/healthz</code></div>
        <div><span>最近检查</span><code>{{ checkedAt }}</code></div>
      </div>

      <nav class="service-tabs" aria-label="服务页面" role="tablist">
        <button type="button" class="service-tab" :class="{ active: activeTab === 'rtk' }" :aria-selected="activeTab === 'rtk'" role="tab" @click="activeTab = 'rtk'">
          RTK
        </button>
        <button type="button" class="service-tab" :class="{ active: activeTab === 'snip' }" :aria-selected="activeTab === 'snip'" role="tab" @click="activeTab = 'snip'">
          snip
        </button>
        <button type="button" class="service-tab" :class="{ active: activeTab === 'llmtrim' }" :aria-selected="activeTab === 'llmtrim'" role="tab" @click="activeTab = 'llmtrim'">
          llmtrim
        </button>
        <button type="button" class="service-tab" :class="{ active: activeTab === 'gortex' }" :aria-selected="activeTab === 'gortex'" role="tab" @click="activeTab = 'gortex'">
          Gortex
        </button>
      </nav>

      <section v-if="activeTab === 'rtk'" class="llmtrim-section rtk-section" aria-labelledby="rtk-title" role="tabpanel">
        <div class="llmtrim-heading">
          <div><p class="eyebrow">RTK CLI</p><h2 id="rtk-title">RTK 管理</h2></div>
          <div class="llmtrim-actions">
            <span class="daemon-badge" :class="{ running: rtk.running, attention: rtk.activationState === 'attention', conflict: rtk.activationState === 'conflict' }">{{ activationLabel(rtk, '已激活') }}</span>
            <button type="button" :class="{ stop: rtk.running || rtk.desiredRunning }" :disabled="rtk.loading || rtk.working || ((!rtk.running && !rtk.desiredRunning) && rtkBlocked) || !rtk.installed" @click="controlRTK">{{ rtk.working ? '处理中…' : ((rtk.running || rtk.desiredRunning) ? '停止' : '启动') }}</button>
            <button type="button" :disabled="rtk.loading || rtkInstallWorking || rtkDeleteWorking" @click="loadRTKStatus">{{ rtk.loading ? '读取中…' : '刷新状态' }}</button>
          </div>
        </div>
        <div class="llmtrim-install-control">
          <label for="rtk-version"><strong>RTK 版本</strong><span>安装只保存 RTK-AI\rtk.exe；点击启动后才配置 PATH，并按已检测到的平台写入 Codex AGENTS.md、Claude Code CLAUDE.md 规则及 Claude/Copilot/Cursor Hook。</span></label>
          <div class="llmtrim-install-row">
            <select id="rtk-version" v-model="selectedRTKVersion" :disabled="rtkBlocked || rtk.running || rtkReleases.loading || rtkInstallWorking" @change="handleRTKVersionChange">
              <option value="">{{ rtkReleases.loaded ? '请选择版本' : '点击右侧加载版本' }}</option>
              <option v-for="release in rtkReleases.items" :key="release.tag_name" :value="release.tag_name" :disabled="!release.available">{{ release.tag_name }}{{ release.prerelease ? '（预发布）' : '' }}{{ !release.available ? '（无 Windows x64 包）' : '' }}</option>
              <option v-if="rtkReleases.hasMore && rtkReleases.loaded" value="__load_more__">加载更多…</option>
            </select>
            <button type="button" :class="{ 'is-loading': rtkReleases.loading }" :disabled="rtkBlocked || rtkReleases.loading" @click="loadRTKReleases()">{{ rtkReleases.loading ? '加载中…' : (rtkReleases.loaded ? '刷新' : '加载') }}</button>
            <button type="button" class="install-button" :disabled="rtkBlocked || rtkInstallWorking || rtkDeleteWorking || rtkReleases.loading || !selectedRTKVersion || selectedRTKVersion === '__load_more__'" @click="installRTK">{{ rtkInstallWorking ? '安装中…' : '安装' }}</button>
            <button type="button" class="delete-button" :disabled="rtkBlocked || rtk.running || (!rtk.installed && !rtk.directoryExists && !rtk.userPath && !rtk.systemPath && !rtk.codexConfigured && !rtk.codexResidual && !rtk.claudeConfigured && !rtk.claudeResidual && !rtk.copilotConfigured && !rtk.cursorConfigured) || rtkInstallWorking || rtkDeleteWorking" @click="requestUninstallRTK">{{ rtkDeleteWorking ? '删除中…' : '删除' }}</button>
          </div>
        </div>
        <div class="llmtrim-control">
            <label><strong>接入状态</strong><span>“已激活”只表示本次激活流程已完成，不代表四个平台都已接入。Codex 状态仅检查 AGENTS.md 规则段，不读取个性化界面文本；RTK 没有后台进程，请以下方逐项状态为准。</span></label>
            <p class="daemon-meta">路径 {{ rtk.path || 'RTK-AI\\rtk.exe' }}</p>
            <p class="daemon-meta version-meta">当前版本 {{ rtk.version || '未记录' }}</p>
            <p class="daemon-meta">用户 PATH {{ rtk.userPath ? '已配置' : '未配置' }} · 系统 PATH {{ rtk.systemPath ? '已配置' : '未配置' }}</p>
            <p class="daemon-meta">Codex {{ rtk.codexAvailable ? (rtk.codexResidual ? 'AGENTS.md 规则待处理' : (rtk.codexConfigured ? 'AGENTS.md 规则已集成' : 'AGENTS.md 规则未集成')) : '未检测到 AGENTS.md' }} · {{ rtkClaudeStatusLabel() }}</p>
            <p class="daemon-meta">Cursor {{ rtk.cursorAvailable ? (rtk.cursorConfigured ? 'Hook 已配置' : 'Hook 未配置') : '未检测到' }} · Copilot {{ rtk.copilotAvailable ? (rtk.copilotConfigured ? 'Hook 已配置' : 'Hook 未配置') : '未检测到' }}</p>
          </div>
        <p v-if="rtk.modifiedAgents.length" class="daemon-meta modified-agents">当前完整受管接入：{{ formatModifiedAgents(rtk.modifiedAgents) }}</p>
        <p v-if="rtkNotice" class="notice">{{ rtkNotice }}</p>
      </section>
      <section v-else-if="activeTab === 'snip'" class="llmtrim-section rtk-section" aria-labelledby="snip-title" role="tabpanel">
        <div class="llmtrim-heading">
          <div><p class="eyebrow">SNIP HOOKS</p><h2 id="snip-title">snip 管理</h2></div>
          <div class="llmtrim-actions">
            <span class="daemon-badge" :class="{ running: snip.running, attention: snip.activationState === 'attention', conflict: snip.activationState === 'conflict' }">{{ activationLabel(snip) }}</span>
            <button type="button" :class="{ stop: snip.running || snip.desiredRunning || snip.cleanupRequired }" :disabled="snip.loading || snip.working || ((!snip.running && !snip.desiredRunning && !snip.cleanupRequired) && snipBlocked) || !snip.installed" @click="controlSnip">{{ snip.working ? '处理中…' : ((snip.running || snip.desiredRunning) ? '停止' : (snip.cleanupRequired ? '清理' : '启动')) }}</button>
            <button type="button" :disabled="snip.loading || snipInstallWorking || snipDeleteWorking" @click="loadSnipStatus">{{ snip.loading ? '读取中…' : '刷新状态' }}</button>
          </div>
        </div>
        <div class="llmtrim-install-control">
          <label for="snip-version"><strong>snip 版本</strong><span>安装到 code-Manager.exe 同级 Snip\snip.exe；启动时才按用户级 Agent 目录调用官方 Hook 初始化。</span></label>
          <div class="llmtrim-install-row">
            <select id="snip-version" v-model="selectedSnipVersion" :disabled="snipBlocked || snip.running || snip.desiredRunning || snipReleases.loading || snipInstallWorking" @change="handleSnipVersionChange">
              <option value="">{{ snipReleases.loaded ? '请选择版本' : '点击右侧加载版本' }}</option>
              <option v-for="release in snipReleases.items" :key="release.tag_name" :value="release.tag_name" :disabled="!release.available">{{ release.tag_name }}{{ release.prerelease ? '（预发布）' : '' }}{{ !release.available ? '（无 Windows x64 包）' : '' }}</option>
              <option v-if="snipReleases.hasMore && snipReleases.loaded" value="__load_more__">加载更多…</option>
            </select>
            <button type="button" :class="{ 'is-loading': snipReleases.loading }" :disabled="snipBlocked || snipReleases.loading" @click="loadSnipReleases()">{{ snipReleases.loading ? '加载中…' : (snipReleases.loaded ? '刷新' : '加载') }}</button>
            <button type="button" class="install-button" :disabled="snipBlocked || snip.running || snip.desiredRunning || snipInstallWorking || snipDeleteWorking || snipReleases.loading || !selectedSnipVersion || selectedSnipVersion === '__load_more__'" @click="installSnip">{{ snipInstallWorking ? '安装中…' : '安装' }}</button>
            <button type="button" class="delete-button" :disabled="snipBlocked || snip.running || snip.desiredRunning || snipInstallWorking || snipDeleteWorking || (!snip.installed && !snip.directoryExists)" @click="requestUninstallSnip">{{ snipDeleteWorking ? '删除中…' : '删除' }}</button>
          </div>
        </div>
        <div class="llmtrim-control">
          <label><strong>Hook 状态</strong><span>启动和停止会统一检查 Codex、Claude Code、Cursor、GitHub Copilot；仅接入存在的用户级目录，不写入项目级文件或第三方插件。</span></label>
          <p class="daemon-meta version-meta">当前版本 {{ snip.version || '未记录' }}</p>
          <p class="daemon-meta">路径 {{ snip.path || 'Snip\\snip.exe' }} · 用户 PATH {{ snip.userPath ? '已配置' : '未配置' }} · 系统 PATH {{ snip.systemPath ? '已配置' : '未配置' }}</p>
          <p class="daemon-meta" v-if="snip.agents.length">{{ snip.agents.map(agentStatusLabel).join(' · ') }}</p>
          <p class="daemon-meta" v-else>未检测到可接入的用户级 Agent 目录。</p>
          <p class="daemon-meta">信任状态与 Snip 启动相互独立；Claude Code、Cursor、GitHub Copilot 不使用这份 Codex 信任记录。需要添加信任时，在 PowerShell 的 Hooks need review 界面选择第 2 项，然后按键盘 Enter（回车）。不是输入数字 2。</p>
          <p v-if="snip.trustNotice" class="notice">{{ snip.trustNotice }}</p>
          <div v-if="snip.trustRequired || snip.trustStatus === 'trusted'" class="trust-guide">
            <div class="trust-guide-heading">
              <strong>Codex Hook 信任</strong>
              <div class="trust-guide-actions">
                <span class="daemon-badge" :class="{ running: snip.trustStatus === 'trusted', attention: snip.trustStatus !== 'trusted' }">
                  {{ snip.trustStatus === 'trusted' ? '已信任' : (snip.trustStatus === 'disabled' ? '已关闭' : '待人工确认') }}
                </span>
                <button v-if="snip.trustStatus === 'untrusted' || snip.trustStatus === 'unknown'" type="button" class="trust-button" :disabled="snipTrustWorking || !snip.installed" @click="addSnipTrust">
                  {{ snipTrustWorking ? '打开中…' : '添加信任' }}
                </button>
              </div>
            </div>
            <p v-if="snip.trustCommand" class="daemon-meta">点击“添加信任”后，PowerShell 将使用以下动态命令启动 Codex：</p>
            <code v-if="snip.trustCommand" class="trust-command">{{ snip.trustCommand }}</code>
            <ol v-if="snip.trustSteps.length" class="trust-steps">
              <li v-for="step in snip.trustSteps" :key="step">{{ step }}</li>
            </ol>
          </div>
        </div>
        <p v-if="snip.modifiedAgents.length" class="daemon-meta modified-agents">已修改 {{ formatModifiedAgents(snip.modifiedAgents) }}</p>
        <p v-if="snipNotice" class="notice">{{ snipNotice }}</p>
      </section>
      <section v-else-if="activeTab === 'llmtrim'" class="llmtrim-section" aria-labelledby="llmtrim-title" role="tabpanel">
        <div class="llmtrim-heading">
          <div>
            <p class="eyebrow">LLMTRIM DAEMON</p>
            <h2 id="llmtrim-title">llmtrim 控制</h2>
          </div>
          <div class="llmtrim-actions">
            <span class="daemon-badge" :class="{ running: llmtrim.running, attention: llmtrim.activationState === 'attention' }">
              {{ activationLabel(llmtrim) }}
            </span>
            <button type="button" :class="{ stop: llmtrim.running || llmtrim.trayRunning }" :disabled="llmtrim.loading || llmtrim.commandInFlight || !llmtrimCanControl" @click="controlLLMTrim(llmtrim.running || llmtrim.trayRunning ? 'stop' : 'start')">
              {{ llmtrimActionLabel() }}
            </button>
            <button type="button" :class="{ stop: llmtrimLogViewer.showing }" :disabled="llmtrim.loading || llmtrim.commandInFlight || llmtrimLogViewer.loading || llmtrimLogViewer.working || (!llmtrimLogViewer.showing && !llmtrim.installed)" @click="controlLLMTrimLogViewer(llmtrimLogViewer.showing ? 'hide' : 'show')">
              {{ llmtrimLogViewer.working ? '处理中…' : (llmtrimLogViewer.showing ? '关闭日志' : '显示日志') }}
            </button>
            <button type="button" :disabled="llmtrim.loading || llmtrim.commandInFlight" @click="loadLLMTrimStatus">
              {{ llmtrim.loading ? '读取中…' : '刷新状态' }}
            </button>
          </div>
        </div>
        <p v-if="llmtrimLogNotice" class="notice llmtrim-log-notice">{{ llmtrimLogNotice }}</p>
        <div class="llmtrim-install-control">
          <label for="llmtrim-version">
            <strong>上游版本</strong>
            <span>点击加载后才从 GitHub 获取版本；每次加载 5 个，选择“加载更多”继续获取。</span>
          </label>
          <div class="llmtrim-install-row">
            <select id="llmtrim-version" v-model="selectedLLMTrimVersion" :disabled="llmtrimReleases.loading || llmtrimInstallWorking" @change="handleLLMTrimVersionChange">
              <option value="">{{ llmtrimReleases.loaded ? '请选择版本' : '点击右侧加载版本' }}</option>
              <option v-for="release in llmtrimReleases.items" :key="release.tag_name" :value="release.tag_name" :disabled="!release.available">
                {{ release.tag_name }}{{ release.prerelease ? '（预发布）' : '' }}{{ !release.available ? '（无 Windows x64 包）' : '' }}
              </option>
              <option v-if="llmtrimReleases.hasMore && llmtrimReleases.loaded" value="__load_more__">加载更多…</option>
            </select>
            <button type="button" :class="{ 'is-loading': llmtrimReleases.loading }" :disabled="llmtrimReleases.loading" @click="loadLLMTrimReleases()">
              {{ llmtrimReleases.loading ? '加载中…' : (llmtrimReleases.loaded ? '刷新' : '加载') }}
            </button>
            <button type="button" class="install-button" :disabled="llmtrimInstallWorking || llmtrimDeleteWorking || llmtrimReleases.loading || !selectedLLMTrimVersion || selectedLLMTrimVersion === '__load_more__'" @click="installLLMTrim">
              {{ llmtrimInstallWorking ? '安装中…' : '安装' }}
            </button>
            <button type="button" class="delete-button" :disabled="llmtrimInstallWorking || llmtrimDeleteWorking || !llmtrim.residual" @click="requestUninstallLLMTrim">
              {{ llmtrimDeleteWorking ? '删除中…' : '删除' }}
            </button>
          </div>
        </div>
        <div class="llmtrim-control">
          <label>
            <strong>Daemon 状态</strong>
            <span>运行中仅在配置路径对应的 llmtrim.exe 与 127.0.0.1:43117 同时确认后显示；停止会清理 llmtrim 的自启动和用户环境。</span>
          </label>
          <p class="daemon-meta version-meta">当前版本 {{ llmtrim.version || '未记录' }}</p>
          <p class="daemon-meta">路径 {{ llmtrim.path || '未记录' }}</p>
          <p class="daemon-meta">daemon {{ llmtrim.running ? '已确认监听' : (llmtrim.trayRunning ? '已停止，tray 仍在运行' : '已停止') }} · 恢复标记 {{ llmtrim.desiredRunning ? '已请求' : '未请求' }}</p>
          <p class="daemon-meta">端口 {{ llmtrimProcess.port || '127.0.0.1:43117' }}<span v-if="llmtrimProcess.id"> · PID {{ llmtrimProcess.id }}</span><span v-if="llmtrim.trayRunning"> · tray PID {{ llmtrim.trayProcessId }}</span></p>
          <p class="daemon-meta">受管安装目录 {{ llmtrim.directoryExists ? '存在' : '不存在' }} · 状态目录 {{ llmtrim.stateDirExists ? '存在' : '不存在' }}<span v-if="llmtrim.running"> · Windows 接管 {{ llmtrim.configured ? '已验证' : '待处理' }}</span></p>
          <p v-if="llmtrim.statusMessage" class="daemon-meta">{{ llmtrim.statusMessage }}</p>
          <p v-if="llmtrimNotice" class="notice">{{ llmtrimNotice }}</p>
        </div>
      </section>
      <section v-else-if="activeTab === 'gortex'" class="llmtrim-section" aria-labelledby="gortex-title" role="tabpanel">
        <div class="llmtrim-heading">
          <div>
            <p class="eyebrow">GORTEX MCP</p>
            <h2 id="gortex-title">Gortex 管理</h2>
          </div>
          <div class="llmtrim-actions">
            <span class="daemon-badge" :class="{ running: gortex.running, attention: gortex.activationState === 'attention' }">
              {{ activationLabel(gortex, 'daemon 运行中') }}
            </span>
            <button type="button" :class="{ stop: gortex.running }" :disabled="gortex.loading || gortexBusy || !gortex.managedInstalled" @click="controlGortex(gortex.running ? 'stop' : 'start')">
              {{ gortex.working ? '处理中…' : (!gortex.managedInstalled ? '仅检测' : (gortex.running ? '停止 daemon' : '启动 daemon')) }}
            </button>
            <button type="button" :disabled="gortex.loading || gortexBusy" @click="loadGortexStatus">
              {{ gortex.loading ? '读取中…' : '刷新状态' }}
            </button>
          </div>
        </div>
        <div class="llmtrim-install-control">
          <label>
            <strong>Gortex 程序</strong>
            <span>优先使用 code-Manager.exe 同级的 Gortex 受管目录；安装来自远端 Windows x64 ZIP，安装后自动写入用户和系统 PATH。</span>
          </label>
          <code class="gortex-path">{{ gortex.path || '未检测到 gortex.exe' }}</code>
          <div class="llmtrim-install-row gortex-actions-row">
            <select id="gortex-version" v-model="selectedGortexVersion" :disabled="gortexReleases.loading || gortexVersionActionBlocked" @change="handleGortexVersionChange">
              <option value="">{{ gortexReleases.loaded ? '请选择版本' : '点击加载版本' }}</option>
              <option v-for="release in gortexReleases.items" :key="release.tag_name" :value="release.tag_name" :disabled="!release.available">
                {{ release.tag_name }}{{ gortexReleaseIsCurrent(release.tag_name) ? '（当前）' : '' }}{{ release.prerelease ? '（预发布）' : '' }}{{ !release.available ? '（无 Windows x64 包）' : '' }}
              </option>
              <option v-if="gortexReleases.hasMore && gortexReleases.loaded" value="__load_more__">加载更多…</option>
            </select>
            <button type="button" :class="{ 'is-loading': gortexReleases.loading }" :disabled="gortexReleases.loading || gortexVersionActionBlocked" @click="loadGortexReleases">
              {{ gortexReleases.loading ? '加载中…' : (gortexReleases.loaded ? '刷新版本' : '加载版本') }}
            </button>
            <button type="button" class="install-button" :disabled="gortexVersionActionBlocked || gortex.loading || !gortex.statusKnown || gortexSelectedVersionIsCurrent || !selectedGortexVersion || selectedGortexVersion === '__load_more__'" @click="requestInstallGortex">
              {{ gortexSelectedVersionIsCurrent ? '已是当前版本' : (gortex.installWorking ? '安装中…' : (gortex.managedInstalled ? '升级 ZIP' : '安装 ZIP')) }}
            </button>
            <button type="button" :disabled="gortexBusy || !gortex.managedInstalled" @click="registerGortex">注册 MCP</button>
            <button type="button" :disabled="gortexBusy || !gortex.integrationPresent" @click="removeGortex">移除 MCP</button>
            <button type="button" class="delete-button" :disabled="gortexUninstallBlocked || !gortex.integrationPresent" @click="requestUninstallGortex">{{ gortex.installing || gortex.installWorking ? '安装中…' : (gortex.running ? '运行中不可卸载' : ((!gortex.statusKnown || gortex.loading) ? '确认状态…' : '卸载')) }}</button>
            <button type="button" :disabled="gortexBusy || !gortex.managedInstalled" @click="runGortexDiagnostics">{{ gortexDiagnostics.loading ? '验证中…' : '验证 doctor/status' }}</button>
          </div>
        </div>
        <div class="llmtrim-control">
          <label>
            <strong>平台接入</strong>
            <span>注册和移除只处理名为 gortex 的 MCP 项，不会删除其他 MCP。track 需要 daemon 建图，若 daemon 已停止会按需启动；不会在后台自动深度审查所有函数。</span>
          </label>
          <p class="daemon-meta version-meta">当前版本 {{ gortex.version || '未记录' }}</p>
           <p class="daemon-meta">daemon {{ gortex.managedInstalled ? (gortex.running ? '受管运行中' : '受管已停止') : '未安装受管版本' }}<span v-if="gortex.processId"> · PID {{ gortex.processId }}</span> · Gortex 进程 {{ gortex.anyProcessRunning ? (gortex.unmanagedProcessRunning ? '有外部/旧路径进程' : '有受管 MCP 或 daemon 进程') : '无' }}</p>
          <p class="daemon-meta">PATH 用户 {{ gortex.userPath ? '已配置' : '未配置' }} · 系统 {{ gortex.systemPath ? '已配置' : '未配置' }}</p>
           <p class="daemon-meta">Codex {{ gortex.codexAvailable ? (gortex.codexConfigured ? (gortex.codexComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到' }} · 提示词 {{ gortexArtifactLabel(gortex.codexPrompt, gortex.codexPromptComplete) }} · Hook {{ gortexArtifactLabel(gortex.codexHook, gortex.codexHookComplete) }}</p>
           <p class="daemon-meta">Claude Code {{ gortex.claudeAvailable ? (gortex.claudeConfigured ? (gortex.claudeComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到' }} · 提示词 {{ gortexArtifactLabel(gortex.claudePrompt, gortex.claudePromptComplete) }} · Hook {{ gortexArtifactLabel(gortex.claudeHook, gortex.claudeHookComplete) }}</p>
          <div class="gortex-platform-list">
            <p class="daemon-meta">Cursor {{ gortex.cursorAvailable ? (gortex.cursorConfigured ? (gortex.cursorComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到用户配置目录' }} · 项目规则 {{ gortexArtifactLabel(gortex.cursorPrompt, gortex.cursorPromptComplete) }}</p>
            <p class="daemon-meta">GitHub Copilot CLI {{ gortex.copilotAvailable ? (gortex.copilotConfigured ? (gortex.copilotComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到用户配置目录' }} · 提示词 {{ gortexArtifactLabel(gortex.copilotPrompt, gortex.copilotPromptComplete) }} · Hook {{ gortexArtifactLabel(gortex.copilotHook, gortex.copilotHookComplete) }}</p>
            <p class="daemon-meta">OpenCode {{ gortex.openCodeAvailable ? (gortex.openCodeConfigured ? (gortex.openCodeComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到' }} · 提示词 {{ gortexArtifactLabel(gortex.openCodePrompt, gortex.openCodePromptComplete) }} · Hook {{ gortex.openCodeAvailable ? (gortex.openCodeHook ? (gortex.openCodeHookComplete ? '插件已注册' : '插件配置残缺') : '插件未注册') : '未检测到' }}</p>
            <p class="daemon-meta">Google Antigravity {{ gortex.antigravityAvailable ? (gortex.antigravityConfigured ? (gortex.antigravityComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到' }} · GEMINI.md {{ gortexArtifactLabel(gortex.antigravityPrompt, gortex.antigravityPromptComplete) }} · Hook {{ gortex.antigravityAvailable ? (gortex.antigravityHook ? (gortex.antigravityHookComplete ? '已注册' : '配置残缺') : '未注册') : '未检测到' }}</p>
            <p class="daemon-meta">Gemini CLI {{ gortex.geminiAvailable ? (gortex.geminiConfigured ? (gortex.geminiComplete ? 'MCP 已注册' : 'MCP 配置残缺') : 'MCP 未注册') : '未检测到' }} · GEMINI.md {{ gortexArtifactLabel(gortex.geminiPrompt, gortex.geminiPromptComplete) }} · Hook {{ gortex.geminiAvailable ? (gortex.geminiHook ? (gortex.geminiHookComplete ? '已注册' : '配置残缺') : '未注册') : '未检测到' }}</p>
          </div>
          <p v-if="gortex.codexTrustRequired || gortex.codexTrustStatus === 'trusted'" class="daemon-meta">Codex Hook 信任：{{ gortex.codexTrustStatus === 'trusted' ? '已信任' : '需要在 /hooks 中审核' }} <button v-if="gortex.codexTrustRequired" type="button" :disabled="gortexBusy" @click="trustGortexCodex">打开信任审核</button></p>
          <p class="daemon-meta">项目级 MCP {{ gortex.projectMCPEnabled ? '已启用' : '未启用' }} · 已绑定 {{ gortex.projectMCPProjects.length }} 个项目</p>
        </div>
        <div class="llmtrim-install-control">
          <label for="gortex-project-path">
            <strong>项目代码图谱</strong>
            <span>输入项目绝对路径后执行 track；取消 track 只解除 Gortex 对该项目的索引关联，不删除项目文件。</span>
          </label>
          <div class="setting-control">
            <input id="gortex-project-path" v-model="gortexProjectPath" spellcheck="false" autocomplete="off" placeholder="D:\\你的项目所在的目录" />
            <button type="button" :disabled="gortexBusy || !gortex.managedInstalled" @click="trackGortexProject">track</button>
          </div>
          <div v-if="gortex.trackedProjects.length" class="gortex-project-list">
            <div v-for="project in gortex.trackedProjects" :key="project" class="gortex-project-item">
              <code>{{ project }}</code>
              <button type="button" :disabled="gortexBusy || !gortex.managedInstalled" @click="untrackGortexProject(project)">untrack</button>
            </div>
          </div>
          <p v-else class="daemon-meta">尚未记录通过本页面 track 的项目。</p>
        </div>
        <div v-if="gortexDiagnostics.doctorOutput || gortexDiagnostics.statusOutput" class="llmtrim-control gortex-diagnostics">
          <label><strong>Gortex 验证结果</strong><span>doctor 负责检查 MCP、Hook 与运行证据；status 展示 daemon 的全部 tracked repositories。</span></label>
          <pre v-if="gortexDiagnostics.doctorOutput">doctor {{ gortexDiagnostics.doctorOk ? '通过' : '返回诊断' }}\n{{ gortexDiagnostics.doctorOutput }}</pre>
          <pre v-if="gortexDiagnostics.statusOutput">status {{ gortexDiagnostics.statusOk ? '通过' : '不可用' }}\n{{ gortexDiagnostics.statusOutput }}</pre>
          <p v-if="gortexDiagnostics.doctorError" class="notice">doctor：{{ gortexDiagnostics.doctorError }}</p>
          <p v-if="gortexDiagnostics.statusError" class="notice">status：{{ gortexDiagnostics.statusError }}</p>
        </div>
        <p v-if="gortex.notice" class="notice">{{ gortex.notice }}</p>
      </section>
      <div v-else class="service-tab-placeholder" aria-hidden="true"></div>

      <section class="settings-section" aria-labelledby="settings-title">
        <div class="section-heading">
          <div>
            <p class="eyebrow">CONFIGURATION</p>
            <h2 id="settings-title">网关配置</h2>
          </div>
          <span v-if="loadingSettings" class="loading-text">读取中…</span>
        </div>

        <div class="setting-card">
          <label for="listen-address">
            <strong>监听地址</strong>
            <span>仅监听 HTTP/1.1；/v1 默认支持 WebSocket Upgrade。填写 IPv4:端口 或 [IPv6]:端口。0.0.0.0 监听所有 IPv4，[::] 监听所有 IPv6，[::1] 仅本机 IPv6。</span>
          </label>
          <div class="setting-control">
            <input id="listen-address" v-model="settings.listenAddress" spellcheck="false" autocomplete="off" @input="sanitizeSettingInput('listenAddress', $event)" @keydown.enter.prevent="discardSettingEnter('listenAddress')" />
            <button type="button" :disabled="saving.listenAddress || loadingSettings" @click="saveSetting('listenAddress', 'listen_address', settings.listenAddress)">
              {{ saving.listenAddress ? '保存中…' : '保存' }}
            </button>
          </div>
          <p v-if="notices.listenAddress" class="notice">{{ notices.listenAddress }}</p>
        </div>

        <div class="setting-card">
          <label for="upstream-base-url">
            <strong>上游 Base URL</strong>
            <span>保存会同步该 URL 的主机名到 llmtrim extra_hosts；llmtrim 需停止后再次启动才会读取新主机并重建 CA。</span>
          </label>
          <div class="setting-control">
            <input id="upstream-base-url" v-model="settings.upstreamBaseURL" type="url" spellcheck="false" autocomplete="url" @input="sanitizeSettingInput('upstreamBaseURL', $event)" @keydown.enter.prevent="discardSettingEnter('upstreamBaseURL')" />
            <button type="button" :disabled="saving.upstreamBaseURL || loadingSettings" @click="saveSetting('upstreamBaseURL', 'upstream_base_url', settings.upstreamBaseURL)">
              {{ saving.upstreamBaseURL ? '保存中…' : '保存' }}
            </button>
          </div>
          <p v-if="notices.upstreamBaseURL" class="notice">{{ notices.upstreamBaseURL }}</p>
        </div>

        <div class="setting-card">
          <label for="upstream-api-key">
            <strong>上游 API Key</strong>
            <span>可直接查看和修改本机当前使用的上游密钥。</span>
          </label>
          <div class="setting-control">
            <input id="upstream-api-key" v-model="apiKey" type="text" spellcheck="false" autocomplete="off" placeholder="输入 API Key" @input="sanitizeSettingInput('upstreamAPIKey', $event)" @keydown.enter.prevent="discardSettingEnter('upstreamAPIKey')" />
            <button type="button" :disabled="saving.upstreamAPIKey || loadingSettings" @click="saveSetting('upstreamAPIKey', 'upstream_api_key', apiKey)">
              {{ saving.upstreamAPIKey ? '保存中…' : '保存' }}
            </button>
          </div>
          <p v-if="notices.upstreamAPIKey" class="notice">{{ notices.upstreamAPIKey }}</p>
        </div>

        <div class="setting-card">
          <label class="switch-control upstream-websocket-toggle" :class="{ disabled: proxyActive || connectionSettingRestartWorking || saving.upstreamWebSocketEnabled || loadingSettings }">
            <span><strong>上游 WS 承载</strong><small>只控制直连上游是否发起 WS 协商；本地 HTTP/WS 监听与 llmtrim 链路始终保持支持。</small></span>
            <input type="checkbox" :checked="settings.upstreamWebSocketEnabled" :disabled="proxyActive || connectionSettingRestartWorking || saving.upstreamWebSocketEnabled || loadingSettings" @change="updateUpstreamWebSocketEnabled($event.target.checked)" />
            <i aria-hidden="true"></i>
          </label>
          <p v-if="notices.upstreamWebSocketEnabled" class="notice">{{ notices.upstreamWebSocketEnabled }}</p>
        </div>

        <div class="setting-card retry-setting-card">
          <div class="retry-config-grid">
            <label class="retry-toggle switch-control" :class="{ disabled: proxyActive || connectionSettingRestartWorking || retrySaving.enabled || loadingSettings }">
              <span>自动重试</span>
              <input type="checkbox" :checked="retry.enabled" :disabled="proxyActive || connectionSettingRestartWorking || retrySaving.enabled || loadingSettings" @change="updateRetryEnabled($event.target.checked)" />
              <i aria-hidden="true"></i>
            </label>
            <label class="retry-field" :class="{ disabled: proxyActive || loadingSettings || retrySaving.count }" for="retry-count">
              <strong>重试次数</strong>
              <input id="retry-count" v-model="retry.count" inputmode="numeric" spellcheck="false" autocomplete="off" placeholder="重试次数" :disabled="proxyActive || loadingSettings || retrySaving.count" @input="sanitizeRetryInput('count', $event)" @blur="normalizeRetryInputForBlur('count'); saveRetrySetting('count', 'retry_count')" />
            </label>
            <label class="retry-field" :class="{ disabled: proxyActive || loadingSettings || retrySaving.intervalSeconds }" for="retry-interval-seconds">
              <strong>间隔时间s</strong>
              <input id="retry-interval-seconds" v-model="retry.intervalSeconds" inputmode="numeric" spellcheck="false" autocomplete="off" placeholder="间隔时间s" :disabled="proxyActive || loadingSettings || retrySaving.intervalSeconds" @input="sanitizeRetryInput('intervalSeconds', $event)" @blur="normalizeRetryInputForBlur('intervalSeconds'); saveRetrySetting('intervalSeconds', 'retry_interval_seconds')" />
            </label>
            <label class="retry-field retry-status-field" :class="{ disabled: proxyActive || loadingSettings || retrySaving.statusCodes }" for="retry-status-codes">
              <strong>自动重试状态码</strong>
              <input id="retry-status-codes" v-model="retry.statusCodes" inputmode="text" spellcheck="false" autocomplete="off" placeholder="自动重试状态码" :disabled="proxyActive || loadingSettings || retrySaving.statusCodes" @input="sanitizeRetryInput('statusCodes', $event)" @blur="normalizeRetryInputForBlur('statusCodes'); saveRetrySetting('statusCodes', 'retry_status_codes')" />
            </label>
          </div>
        </div>
      </section>

      <section class="application-actions" aria-label="程序运行设置">
        <label class="switch-control">
          <span>开机启动</span>
          <input type="checkbox" :checked="startup.enabled" :disabled="startup.working || startup.exiting || loadingSettings" @change="updateStartupSetting('startup_enabled', $event.target.checked)" />
          <i aria-hidden="true"></i>
        </label>
        <label class="switch-control" :class="{ disabled: !startup.enabled }">
          <span>后台运行</span>
          <input type="checkbox" :checked="startup.background" :disabled="!startup.enabled || startup.working || startup.exiting || loadingSettings" @change="updateStartupSetting('background_start', $event.target.checked)" />
          <i aria-hidden="true"></i>
        </label>
        <button type="button" class="exit-button" :disabled="startup.exiting" @click="exitApplication">
          {{ startup.exiting ? '正在退出…' : '停止并退出' }}
        </button>
      </section>
      <p v-if="startupNotice" class="notice startup-notice">{{ startupNotice }}</p>
      <p class="hint">关闭浏览器页面不会停止网关进程。</p>
    </section>
  </main>
  <div v-if="confirmation.open" class="application-confirmation-overlay" @keydown="handleConfirmationKeydown">
    <section class="application-confirmation-dialog" role="alertdialog" aria-modal="true" aria-labelledby="application-confirmation-title" aria-describedby="application-confirmation-description">
      <h2 id="application-confirmation-title">{{ confirmation.title }}</h2>
      <p id="application-confirmation-description">{{ confirmation.description }}</p>
      <div class="application-confirmation-actions">
        <button ref="confirmationCancelButton" type="button" class="confirmation-cancel-button" :disabled="confirmationExecuting" @click="cancelConfirmation">取消</button>
        <button ref="confirmationConfirmButton" type="button" class="confirmation-confirm-button" :disabled="confirmationExecuting" @click="confirmPendingAction">{{ confirmation.confirmLabel }}</button>
      </div>
    </section>
  </div>
  <div v-if="exitState !== 'idle'" class="application-exit-overlay" aria-live="assertive">
    <section class="application-exit-dialog" :class="`state-${exitState}`" role="alertdialog" aria-modal="true" aria-labelledby="application-exit-title" aria-describedby="application-exit-description">
      <span class="application-exit-mark" aria-hidden="true"></span>
      <h2 id="application-exit-title">{{ exitTitle }}</h2>
      <p id="application-exit-description">{{ exitDescription }}</p>
      <ul v-if="exitWarnings.length" class="application-exit-warnings">
        <li v-for="(warning, index) in exitWarnings" :key="`${index}-${warning}`">{{ warning }}</li>
      </ul>
    </section>
  </div>
  <div v-if="applicationUpdateRestarting" class="application-exit-overlay" aria-live="assertive">
    <section class="application-exit-dialog state-stopping" role="alertdialog" aria-modal="true" aria-labelledby="application-update-title" aria-describedby="application-update-description">
      <span class="application-exit-mark" aria-hidden="true"></span>
      <h2 id="application-update-title">正在安装并重启</h2>
      <p id="application-update-description">{{ applicationUpdateNotice || '正在替换 code-Manager.exe；四项工具会保持当前状态。' }}</p>
    </section>
  </div>
</template>
