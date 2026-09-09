package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"
)

type gortexOwnedProjectMCP struct {
	Agent       string `json:"agent"`
	Project     string `json:"project"`
	Path        string `json:"path"`
	Fingerprint string `json:"fingerprint"`
}

var gortexProjectMCPAgents = []string{"codex", "claude", "cursor", "copilot", "opencode", "antigravity", "gemini"}

func gortexDetectedMCPAgents() map[string]bool {
	result := make(map[string]bool, len(gortexProjectMCPAgents))
	for _, agent := range gortexProjectMCPAgents {
		result[agent] = gortexAgentAvailable(agent)
	}
	return result
}

func gortexProjectMCPConfigPath(agent, project string) string {
	project = strings.TrimSpace(project)
	if project == "" {
		return ""
	}
	absolute, err := filepath.Abs(project)
	if err != nil {
		return ""
	}
	project = filepath.Clean(absolute)
	switch agent {
	case "codex":
		return filepath.Join(project, ".codex", "config.toml")
	case "claude":
		return filepath.Join(project, ".mcp.json")
	case "cursor":
		return filepath.Join(project, ".cursor", "mcp.json")
	case "copilot":
		return filepath.Join(project, ".github", "mcp.json")
	case "opencode":
		return filepath.Join(project, "opencode.json")
	case "antigravity":
		return filepath.Join(project, ".agents", "mcp_config.json")
	case "gemini":
		return filepath.Join(project, ".gemini", "settings.json")
	default:
		return ""
	}
}

func gortexProjectMCPKey(agent, project string) string {
	project = strings.TrimSpace(project)
	absolute, err := filepath.Abs(project)
	if err == nil {
		project = filepath.Clean(absolute)
	} else {
		project = filepath.Clean(project)
	}
	return agent + "|" + strings.ToLower(project)
}

func gortexProjectMCPUsesCWD(agent string) bool {
	switch agent {
	case "codex", "opencode", "antigravity", "gemini":
		return true
	default:
		return false
	}
}

func gortexProjectMCPEntry(agent, executable, project string) map[string]any {
	project = filepath.Clean(project)
	var entry map[string]any
	switch agent {
	case "codex":
		entry = gortexCodexMCPEntry(executable)
	case "opencode":
		entry = gortexOpenCodeMCPEntry(executable)
	default:
		entry = gortexMCPEntry(executable, agent == "copilot")
	}
	if gortexProjectMCPUsesCWD(agent) {
		entry["cwd"] = project
	}
	return entry
}

func gortexProjectMCPEntryCWDMatches(value any, project string) bool {
	entry, ok := value.(map[string]any)
	if !ok {
		return false
	}
	cwd, ok := entry["cwd"].(string)
	if !ok || strings.TrimSpace(cwd) == "" {
		return false
	}
	left, leftErr := filepath.Abs(filepath.Clean(cwd))
	right, rightErr := filepath.Abs(filepath.Clean(project))
	if leftErr != nil || rightErr != nil {
		return false
	}
	return strings.EqualFold(filepath.Clean(left), filepath.Clean(right))
}

func gortexProjectMCPEntryLooksManaged(value any, agent, project string) bool {
	if agent == "opencode" {
		if !gortexOpenCodeMCPEntryLooksManaged(value) {
			return false
		}
	} else if !gortexMCPEntryLooksManaged(value) {
		return false
	}
	return !gortexProjectMCPUsesCWD(agent) || gortexProjectMCPEntryCWDMatches(value, project)
}

func gortexProjectMCPRegistrationAllowed(existing, desired any, owned gortexOwnedProjectMCP, agent, project string) bool {
	if gortexFingerprint(existing) == gortexFingerprint(desired) {
		return true
	}
	if owned.Fingerprint != "" {
		return gortexFingerprint(existing) == owned.Fingerprint
	}
	return gortexProjectMCPEntryLooksManaged(existing, agent, project)
}

func gortexProjectMCPRemovalAllowed(existing any, owned gortexOwnedProjectMCP, recorded bool, agent, project string) bool {
	if recorded && owned.Fingerprint != "" {
		return gortexFingerprint(existing) == owned.Fingerprint
	}
	return gortexProjectMCPEntryLooksManaged(existing, agent, project)
}

func gortexEnsureProjectMCPOwnership(value *gortexMCPOwnership) {
	if value.ProjectMCP == nil {
		value.ProjectMCP = map[string]gortexOwnedProjectMCP{}
	}
}

func gortexProjectMCPRecord(ownership *gortexMCPOwnership, agent, project, path string, value any) {
	gortexEnsureProjectMCPOwnership(ownership)
	ownership.ProjectMCP[gortexProjectMCPKey(agent, project)] = gortexOwnedProjectMCP{
		Agent:       agent,
		Project:     filepath.Clean(project),
		Path:        path,
		Fingerprint: gortexFingerprint(value),
	}
}

func gortexDeleteProjectMCPRecord(ownership *gortexMCPOwnership, agent, project string) {
	gortexEnsureProjectMCPOwnership(ownership)
	delete(ownership.ProjectMCP, gortexProjectMCPKey(agent, project))
}

func gortexProjectMCPJSONObject(path string) (map[string]any, error) {
	root := map[string]any{}
	if _, err := os.Stat(path); err == nil {
		return readGortexJSONObject(path)
	} else if errors.Is(err, os.ErrNotExist) {
		return root, nil
	} else {
		return nil, err
	}
}

func updateProjectJSONMCPConfigOwned(agent, project, executable string, remove bool) (bool, error) {
	path := gortexProjectMCPConfigPath(agent, project)
	if path == "" {
		return false, nil
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}, ProjectMCP: map[string]gortexOwnedProjectMCP{}}
	}
	gortexEnsureProjectMCPOwnership(&ownership)
	key := gortexProjectMCPKey(agent, project)
	root, err := gortexProjectMCPJSONObject(path)
	if err != nil {
		return false, err
	}
	if remove && !fileExists(path) {
		if ownershipErr == nil {
			if _, exists := ownership.ProjectMCP[key]; exists {
				delete(ownership.ProjectMCP, key)
				return true, writeGortexOwnership(ownership)
			}
		}
		return false, nil
	}
	servers, err := jsonServers(root, path)
	if err != nil {
		return false, err
	}
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, recorded := ownership.ProjectMCP[key]; recorded {
					delete(ownership.ProjectMCP, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		record, recorded := ownership.ProjectMCP[key]
		if !gortexProjectMCPRemovalAllowed(existing, record, recorded, agent, project) {
			return false, fmt.Errorf("%s 项目级 gortex MCP 已被用户修改，已保留", agent)
		}
		delete(servers, gortexMCPName)
		gortexDeleteProjectMCPRecord(&ownership, agent, project)
	} else {
		entry := gortexProjectMCPEntry(agent, executable, project)
		if exists {
			record := ownership.ProjectMCP[key]
			if !gortexProjectMCPRegistrationAllowed(existing, entry, record, agent, project) {
				return false, fmt.Errorf("%s 项目中已存在用户配置的 gortex MCP，已保留", agent)
			}
		}
		servers[gortexMCPName] = entry
		gortexProjectMCPRecord(&ownership, agent, project, path, entry)
	}
	if len(servers) == 0 {
		delete(root, "mcpServers")
	}
	if err := writeGortexJSONObject(path, root); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}

func updateProjectOpenCodeMCPConfigOwned(project, executable string, remove bool) (bool, error) {
	path := gortexProjectMCPConfigPath("opencode", project)
	if path == "" {
		return false, nil
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}, ProjectMCP: map[string]gortexOwnedProjectMCP{}}
	}
	gortexEnsureProjectMCPOwnership(&ownership)
	key := gortexProjectMCPKey("opencode", project)
	root, err := gortexProjectMCPJSONObject(path)
	if err != nil {
		return false, err
	}
	if remove && !fileExists(path) {
		if ownershipErr == nil {
			if _, exists := ownership.ProjectMCP[key]; exists {
				delete(ownership.ProjectMCP, key)
				return true, writeGortexOwnership(ownership)
			}
		}
		return false, nil
	}
	servers := map[string]any{}
	if value, exists := root["mcp"]; exists {
		var ok bool
		servers, ok = value.(map[string]any)
		if !ok {
			return false, fmt.Errorf("%s 的 mcp 不是对象，拒绝覆盖用户配置", path)
		}
	}
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, recorded := ownership.ProjectMCP[key]; recorded {
					delete(ownership.ProjectMCP, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		record, recorded := ownership.ProjectMCP[key]
		if !gortexProjectMCPRemovalAllowed(existing, record, recorded, "opencode", project) {
			return false, errors.New("OpenCode 项目级 gortex MCP 已被用户修改，已保留")
		}
		delete(servers, gortexMCPName)
		gortexDeleteProjectMCPRecord(&ownership, "opencode", project)
	} else {
		entry := gortexProjectMCPEntry("opencode", executable, project)
		if exists {
			record := ownership.ProjectMCP[key]
			if !gortexProjectMCPRegistrationAllowed(existing, entry, record, "opencode", project) {
				return false, errors.New("OpenCode 项目中已存在用户配置的 gortex MCP，已保留")
			}
		}
		servers[gortexMCPName] = entry
		gortexProjectMCPRecord(&ownership, "opencode", project, path, entry)
	}
	if len(servers) == 0 {
		delete(root, "mcp")
	} else {
		root["mcp"] = servers
	}
	if err := writeGortexJSONObject(path, root); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}

func updateProjectCodexMCPConfigOwned(project, executable string, remove bool) (bool, error) {
	path := gortexProjectMCPConfigPath("codex", project)
	if path == "" {
		return false, nil
	}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		if !remove {
			return false, ownershipErr
		}
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}, ProjectMCP: map[string]gortexOwnedProjectMCP{}}
	}
	gortexEnsureProjectMCPOwnership(&ownership)
	key := gortexProjectMCPKey("codex", project)
	root := map[string]any{}
	if data, err := os.ReadFile(path); err == nil {
		if _, err := toml.Decode(string(data), &root); err != nil {
			return false, fmt.Errorf("解析 Codex 项目 config.toml 失败: %w", err)
		}
	} else if errors.Is(err, os.ErrNotExist) {
		if remove {
			if ownershipErr == nil {
				if _, exists := ownership.ProjectMCP[key]; exists {
					delete(ownership.ProjectMCP, key)
					return true, writeGortexOwnership(ownership)
				}
			}
			return false, nil
		}
	} else {
		return false, err
	}
	servers := map[string]any{}
	if value, exists := root["mcp_servers"]; exists {
		var ok bool
		servers, ok = value.(map[string]any)
		if !ok {
			return false, errors.New("Codex 项目 mcp_servers 不是对象，拒绝覆盖用户配置")
		}
	}
	existing, exists := servers[gortexMCPName]
	if remove {
		if !exists {
			if ownershipErr == nil {
				if _, recorded := ownership.ProjectMCP[key]; recorded {
					delete(ownership.ProjectMCP, key)
					if err := writeGortexOwnership(ownership); err != nil {
						return false, err
					}
					return true, nil
				}
			}
			return false, nil
		}
		record, recorded := ownership.ProjectMCP[key]
		if !gortexProjectMCPRemovalAllowed(existing, record, recorded, "codex", project) {
			return false, errors.New("Codex 项目级 gortex MCP 已被用户修改，已保留")
		}
		delete(servers, gortexMCPName)
		gortexDeleteProjectMCPRecord(&ownership, "codex", project)
	} else {
		entry := gortexProjectMCPEntry("codex", executable, project)
		if exists {
			record := ownership.ProjectMCP[key]
			if !gortexProjectMCPRegistrationAllowed(existing, entry, record, "codex", project) {
				return false, errors.New("Codex 项目中已存在用户配置的 gortex MCP，已保留")
			}
		}
		servers[gortexMCPName] = entry
		gortexProjectMCPRecord(&ownership, "codex", project, path, entry)
	}
	if len(servers) == 0 {
		delete(root, "mcp_servers")
	} else {
		root["mcp_servers"] = servers
	}
	var encoded strings.Builder
	if err := toml.NewEncoder(&encoded).Encode(root); err != nil {
		return false, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return false, err
	}
	if err := replaceUTF8File(path, []byte(encoded.String())); err != nil {
		return false, err
	}
	if ownershipErr == nil {
		if err := writeGortexOwnership(ownership); err != nil {
			return false, err
		}
	}
	return true, nil
}

func gortexRegisterProjectMCPForProject(project, executable string, available map[string]bool) []string {
	project, err := normalizeGortexProjectPath(project)
	if err != nil {
		return []string{"项目级 MCP: " + err.Error()}
	}
	if !directoryExists(project) {
		return []string{fmt.Sprintf("项目级 MCP: 项目目录不存在，已跳过 %s", project)}
	}
	if available == nil {
		available = gortexDetectedMCPAgents()
	}
	warnings := []string{}
	for _, agent := range gortexProjectMCPAgents {
		if !available[agent] {
			continue
		}
		var err error
		switch agent {
		case "codex":
			_, err = updateProjectCodexMCPConfigOwned(project, executable, false)
		case "opencode":
			_, err = updateProjectOpenCodeMCPConfigOwned(project, executable, false)
		default:
			_, err = updateProjectJSONMCPConfigOwned(agent, project, executable, false)
		}
		if err != nil {
			warnings = append(warnings, agent+" 项目级 MCP: "+err.Error())
		}
	}
	return warnings
}

func gortexMCPProjectsForRegistration(executable string) ([]string, []string) {
	projects := []string{}
	warnings := []string{}
	if registry, err := readGortexProjectRegistry(); err == nil {
		projects = append(projects, registry.Projects...)
	} else {
		warnings = append(warnings, "读取 Gortex 项目记录失败: "+err.Error())
	}
	projects = append(projects, gortexDaemonTrackedProjects(executable)...)
	return normalizeGortexProjects(projects), warnings
}

func gortexEnableAndRegisterProjectMCP(executable string, available map[string]bool) []string {
	warnings := []string{}
	ownership, err := readGortexOwnership()
	if err != nil {
		return []string{"读取 Gortex 项目 MCP 归属失败: " + err.Error()}
	}
	ownership.ProjectMCPEnabled = true
	gortexEnsureProjectMCPOwnership(&ownership)
	if err := writeGortexOwnership(ownership); err != nil {
		return []string{"保存 Gortex 项目 MCP 状态失败: " + err.Error()}
	}
	projects, projectWarnings := gortexMCPProjectsForRegistration(executable)
	warnings = append(warnings, projectWarnings...)
	for _, project := range projects {
		warnings = append(warnings, gortexRegisterProjectMCPForProject(project, executable, available)...)
	}
	if available["cursor"] {
		ownership, err = readGortexOwnership()
		if err != nil {
			warnings = append(warnings, "读取 Cursor 项目提示词归属失败: "+err.Error())
		} else {
			for _, project := range projects {
				if err := gortexRegisterCursorProject(project, &ownership); err != nil {
					warnings = append(warnings, "Cursor 项目规则: "+err.Error())
				}
			}
			if err := writeGortexOwnership(ownership); err != nil {
				warnings = append(warnings, "保存 Cursor 项目提示词归属失败: "+err.Error())
			}
		}
	}
	return warnings
}

func gortexRemoveProjectMCP(executable, project string) []string {
	project, err := normalizeGortexProjectPath(project)
	if err != nil {
		return []string{"项目级 MCP: " + err.Error()}
	}
	warnings := []string{}
	for _, agent := range gortexProjectMCPAgents {
		var err error
		switch agent {
		case "codex":
			_, err = updateProjectCodexMCPConfigOwned(project, executable, true)
		case "opencode":
			_, err = updateProjectOpenCodeMCPConfigOwned(project, executable, true)
		default:
			_, err = updateProjectJSONMCPConfigOwned(agent, project, executable, true)
		}
		if err != nil {
			warnings = append(warnings, agent+" 项目级 MCP: "+err.Error())
		}
	}
	return warnings
}

func gortexProjectMCPProjectsFromOwnership(ownership gortexMCPOwnership) []string {
	projects := []string{}
	for _, record := range ownership.ProjectMCP {
		if strings.TrimSpace(record.Project) != "" {
			projects = append(projects, record.Project)
		}
	}
	return normalizeGortexProjects(projects)
}

func gortexRemoveAllProjectMCP(executable string) []string {
	warnings := []string{}
	ownership, ownershipErr := readGortexOwnership()
	if ownershipErr != nil {
		warnings = append(warnings, "读取 Gortex 项目 MCP 归属失败: "+ownershipErr.Error())
		ownership = gortexMCPOwnership{Platforms: map[string]gortexOwnedMCP{}, Artifacts: map[string]gortexOwnedArtifact{}, ProjectMCP: map[string]gortexOwnedProjectMCP{}}
	}
	projects := gortexProjectMCPProjectsFromOwnership(ownership)
	if registry, err := readGortexProjectRegistry(); err == nil {
		projects = normalizeGortexProjects(append(projects, registry.Projects...))
	} else {
		warnings = append(warnings, "读取 Gortex 项目记录失败: "+err.Error())
	}
	projects = normalizeGortexProjects(append(projects, gortexDaemonTrackedProjects(executable)...))
	for _, project := range projects {
		warnings = append(warnings, gortexRemoveProjectMCP(executable, project)...)
	}
	latest, err := readGortexOwnership()
	if err != nil {
		warnings = append(warnings, "重新读取 Gortex 项目 MCP 归属失败: "+err.Error())
		return warnings
	}
	latest.ProjectMCPEnabled = false
	if err := writeGortexOwnership(latest); err != nil {
		warnings = append(warnings, "清除 Gortex 项目 MCP 状态失败: "+err.Error())
	}
	return warnings
}
