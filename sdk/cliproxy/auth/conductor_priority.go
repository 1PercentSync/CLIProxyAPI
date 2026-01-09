package auth

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/router-for-me/CLIProxyAPI/v6/internal/registry"
	"github.com/router-for-me/CLIProxyAPI/v6/internal/util"
	cliproxyexecutor "github.com/router-for-me/CLIProxyAPI/v6/sdk/cliproxy/executor"
	log "github.com/sirupsen/logrus"
)

// priorityCandidate holds an auth together with its resolved executor for cross-provider selection.
type priorityCandidate struct {
	auth     *Auth
	executor ProviderExecutor
}

// hasCrossProviderPriority checks whether cross-provider priority selection should be used.
// It returns true if there are auths with different priority values across the given providers.
// When all auths have the same priority (including all zeros), it returns false to use the
// original upstream round-robin logic.
func (m *Manager) hasCrossProviderPriority(model string, providers []string) bool {
	if len(providers) == 0 {
		return false
	}

	m.mu.RLock()
	defer m.mu.RUnlock()

	providerSet := make(map[string]struct{}, len(providers))
	for _, p := range providers {
		providerSet[strings.TrimSpace(strings.ToLower(p))] = struct{}{}
	}

	modelKey := strings.TrimSpace(model)
	registryRef := registry.GetGlobalRegistry()

	var firstPriority *int
	for _, auth := range m.auths {
		if auth == nil || auth.Disabled {
			continue
		}
		providerKey := strings.TrimSpace(strings.ToLower(auth.Provider))
		if _, ok := providerSet[providerKey]; !ok {
			continue
		}
		if modelKey != "" && registryRef != nil && !registryRef.ClientSupportsModel(auth.ID, modelKey) {
			continue
		}
		if firstPriority == nil {
			p := auth.Priority
			firstPriority = &p
		} else if auth.Priority != *firstPriority {
			return true
		}
	}
	return false
}

// collectPriorityCandidates gathers all available auths across providers that support the model,
// sorted by priority (descending). Returns nil if no candidates are available.
func (m *Manager) collectPriorityCandidates(model string, providers []string, tried map[string]struct{}) []priorityCandidate {
	providerSet := make(map[string]struct{}, len(providers))
	for _, p := range providers {
		providerSet[strings.TrimSpace(strings.ToLower(p))] = struct{}{}
	}

	modelKey := strings.TrimSpace(model)
	registryRef := registry.GetGlobalRegistry()
	now := time.Now()

	m.mu.RLock()
	defer m.mu.RUnlock()

	candidates := make([]priorityCandidate, 0, len(m.auths))
	for _, auth := range m.auths {
		if auth == nil || auth.Disabled {
			continue
		}
		providerKey := strings.TrimSpace(strings.ToLower(auth.Provider))
		if _, ok := providerSet[providerKey]; !ok {
			continue
		}
		if _, used := tried[auth.ID]; used {
			continue
		}
		if modelKey != "" && registryRef != nil && !registryRef.ClientSupportsModel(auth.ID, modelKey) {
			continue
		}
		// Check if auth is blocked (cooldown, disabled, etc.)
		blocked, _, _ := isAuthBlockedForModel(auth, model, now)
		if blocked {
			continue
		}
		executor, ok := m.executors[auth.Provider]
		if !ok {
			continue
		}
		candidates = append(candidates, priorityCandidate{auth: auth, executor: executor})
	}

	if len(candidates) == 0 {
		return nil
	}

	// Sort by priority descending (higher priority first), then by ID ascending (for deterministic order)
	sort.SliceStable(candidates, func(i, j int) bool {
		if candidates[i].auth.Priority != candidates[j].auth.Priority {
			return candidates[i].auth.Priority > candidates[j].auth.Priority
		}
		// Same priority: sort by ID for deterministic ordering
		return candidates[i].auth.ID < candidates[j].auth.ID
	})

	return candidates
}

// applyPriorityRoundRobin reorders candidates to implement round-robin within each priority level.
// It uses the Manager's providerOffsets map with a special key format to track cursors.
// This ensures load balancing among auths with the same priority.
func (m *Manager) applyPriorityRoundRobin(model string, candidates []priorityCandidate) []priorityCandidate {
	if len(candidates) <= 1 {
		return candidates
	}

	// Group candidates by priority level
	type priorityGroup struct {
		priority int
		start    int // start index in candidates slice
		count    int // number of candidates in this group
	}

	groups := make([]priorityGroup, 0)
	currentPriority := candidates[0].auth.Priority
	currentStart := 0

	for i := 1; i < len(candidates); i++ {
		if candidates[i].auth.Priority != currentPriority {
			groups = append(groups, priorityGroup{
				priority: currentPriority,
				start:    currentStart,
				count:    i - currentStart,
			})
			currentPriority = candidates[i].auth.Priority
			currentStart = i
		}
	}
	// Don't forget the last group
	groups = append(groups, priorityGroup{
		priority: currentPriority,
		start:    currentStart,
		count:    len(candidates) - currentStart,
	})

	// Apply round-robin rotation to each group
	result := make([]priorityCandidate, 0, len(candidates))

	for _, group := range groups {
		if group.count <= 1 {
			// Single candidate, no rotation needed
			result = append(result, candidates[group.start:group.start+group.count]...)
			continue
		}

		// Get cursor for this (model, priority) combination
		key := fmt.Sprintf("priority:%s:%d", model, group.priority)

		m.mu.Lock()
		if m.providerOffsets == nil {
			m.providerOffsets = make(map[string]int)
		}
		offset := m.providerOffsets[key]
		if offset >= 2_147_483_640 {
			offset = 0
		}
		m.providerOffsets[key] = offset + 1
		m.mu.Unlock()

		// Rotate the group starting from offset
		offset = offset % group.count
		groupCandidates := candidates[group.start : group.start+group.count]

		// Append rotated: first from offset to end, then from start to offset
		result = append(result, groupCandidates[offset:]...)
		result = append(result, groupCandidates[:offset]...)
	}

	return result
}

// debugLogPriorityAuthSelection logs auth selection with priority information.
// This follows the upstream debugLogAuthSelection pattern but adds priority info.
func debugLogPriorityAuthSelection(entry *log.Entry, auth *Auth, model string) {
	if !log.IsLevelEnabled(log.DebugLevel) {
		return
	}
	if entry == nil || auth == nil {
		return
	}
	accountType, accountInfo := auth.AccountInfo()
	proxyInfo := auth.ProxyInfo()
	suffix := fmt.Sprintf(" (priority: %d)", auth.Priority)
	if proxyInfo != "" {
		suffix = " " + proxyInfo + suffix
	}
	switch accountType {
	case "api_key":
		entry.Debugf("Use API key %s for model %s%s", util.HideAPIKey(accountInfo), model, suffix)
	case "oauth":
		entry.Debugf("Use OAuth %s for model %s%s", accountInfo, model, suffix)
	}
}

// executeWithPriority performs execution using cross-provider priority-based auth selection.
// It collects all available auths across providers, sorts them by priority, and tries them
// in priority order until one succeeds.
func (m *Manager) executeWithPriority(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options) (cliproxyexecutor.Response, error) {
	routeModel := req.Model
	tried := make(map[string]struct{})
	var lastErr error

	retryTimes, maxWait := m.retrySettings()
	attempts := retryTimes + 1
	if attempts < 1 {
		attempts = 1
	}

	for attempt := 0; attempt < attempts; attempt++ {
		resp, err := m.executePriorityOnce(ctx, providers, req, opts, routeModel, tried)
		if err == nil {
			return resp, nil
		}
		lastErr = err
		wait, shouldRetry := m.shouldRetryAfterError(err, attempt, attempts, providers, routeModel, maxWait)
		if !shouldRetry {
			break
		}
		if errWait := waitForCooldown(ctx, wait); errWait != nil {
			return cliproxyexecutor.Response{}, errWait
		}
	}
	if lastErr != nil {
		return cliproxyexecutor.Response{}, lastErr
	}
	return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
}

func (m *Manager) executePriorityOnce(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, routeModel string, tried map[string]struct{}) (cliproxyexecutor.Response, error) {
	candidates := m.collectPriorityCandidates(routeModel, providers, tried)
	if len(candidates) == 0 {
		return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
	}

	// Reorder candidates to implement round-robin within the same priority level.
	// This ensures load balancing among auths with identical priority.
	candidates = m.applyPriorityRoundRobin(routeModel, candidates)

	var lastErr error
	for _, candidate := range candidates {
		auth := candidate.auth
		executor := candidate.executor

		entry := logEntryWithRequestID(ctx)
		debugLogPriorityAuthSelection(entry, auth, req.Model)

		tried[auth.ID] = struct{}{}
		execCtx := ctx
		if rt := m.roundTripperFor(auth); rt != nil {
			execCtx = context.WithValue(execCtx, roundTripperContextKey{}, rt)
			execCtx = context.WithValue(execCtx, "cliproxy.roundtripper", rt)
		}
		execReq := req
		execReq.Model, execReq.Metadata = rewriteModelForAuth(routeModel, req.Metadata, auth)
		execReq.Model, execReq.Metadata = m.applyOAuthModelMapping(auth, execReq.Model, execReq.Metadata)

		resp, errExec := executor.Execute(execCtx, auth, execReq, opts)
		result := Result{AuthID: auth.ID, Provider: auth.Provider, Model: routeModel, Success: errExec == nil}
		if errExec != nil {
			result.Error = &Error{Message: errExec.Error()}
			var se cliproxyexecutor.StatusError
			if errors.As(errExec, &se) && se != nil {
				result.Error.HTTPStatus = se.StatusCode()
			}
			if ra := retryAfterFromError(errExec); ra != nil {
				result.RetryAfter = ra
			}
			m.MarkResult(execCtx, result)
			lastErr = errExec
			continue
		}
		m.MarkResult(execCtx, result)
		return resp, nil
	}

	if lastErr != nil {
		return cliproxyexecutor.Response{}, lastErr
	}
	return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
}

// executeCountWithPriority performs token counting using cross-provider priority-based auth selection.
func (m *Manager) executeCountWithPriority(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options) (cliproxyexecutor.Response, error) {
	routeModel := req.Model
	tried := make(map[string]struct{})
	var lastErr error

	retryTimes, maxWait := m.retrySettings()
	attempts := retryTimes + 1
	if attempts < 1 {
		attempts = 1
	}

	for attempt := 0; attempt < attempts; attempt++ {
		resp, err := m.executeCountPriorityOnce(ctx, providers, req, opts, routeModel, tried)
		if err == nil {
			return resp, nil
		}
		lastErr = err
		wait, shouldRetry := m.shouldRetryAfterError(err, attempt, attempts, providers, routeModel, maxWait)
		if !shouldRetry {
			break
		}
		if errWait := waitForCooldown(ctx, wait); errWait != nil {
			return cliproxyexecutor.Response{}, errWait
		}
	}
	if lastErr != nil {
		return cliproxyexecutor.Response{}, lastErr
	}
	return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
}

func (m *Manager) executeCountPriorityOnce(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, routeModel string, tried map[string]struct{}) (cliproxyexecutor.Response, error) {
	candidates := m.collectPriorityCandidates(routeModel, providers, tried)
	if len(candidates) == 0 {
		return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
	}

	// Reorder candidates to implement round-robin within the same priority level.
	candidates = m.applyPriorityRoundRobin(routeModel, candidates)

	var lastErr error
	for _, candidate := range candidates {
		auth := candidate.auth
		executor := candidate.executor

		entry := logEntryWithRequestID(ctx)
		debugLogPriorityAuthSelection(entry, auth, req.Model)

		tried[auth.ID] = struct{}{}
		execCtx := ctx
		if rt := m.roundTripperFor(auth); rt != nil {
			execCtx = context.WithValue(execCtx, roundTripperContextKey{}, rt)
			execCtx = context.WithValue(execCtx, "cliproxy.roundtripper", rt)
		}
		execReq := req
		execReq.Model, execReq.Metadata = rewriteModelForAuth(routeModel, req.Metadata, auth)
		execReq.Model, execReq.Metadata = m.applyOAuthModelMapping(auth, execReq.Model, execReq.Metadata)

		resp, errExec := executor.CountTokens(execCtx, auth, execReq, opts)
		result := Result{AuthID: auth.ID, Provider: auth.Provider, Model: routeModel, Success: errExec == nil}
		if errExec != nil {
			result.Error = &Error{Message: errExec.Error()}
			var se cliproxyexecutor.StatusError
			if errors.As(errExec, &se) && se != nil {
				result.Error.HTTPStatus = se.StatusCode()
			}
			if ra := retryAfterFromError(errExec); ra != nil {
				result.RetryAfter = ra
			}
			m.MarkResult(execCtx, result)
			lastErr = errExec
			continue
		}
		m.MarkResult(execCtx, result)
		return resp, nil
	}

	if lastErr != nil {
		return cliproxyexecutor.Response{}, lastErr
	}
	return cliproxyexecutor.Response{}, &Error{Code: "auth_not_found", Message: "no auth available"}
}

// executeStreamWithPriority performs streaming execution using cross-provider priority-based auth selection.
func (m *Manager) executeStreamWithPriority(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options) (<-chan cliproxyexecutor.StreamChunk, error) {
	routeModel := req.Model
	tried := make(map[string]struct{})
	var lastErr error

	retryTimes, maxWait := m.retrySettings()
	attempts := retryTimes + 1
	if attempts < 1 {
		attempts = 1
	}

	for attempt := 0; attempt < attempts; attempt++ {
		chunks, err := m.executeStreamPriorityOnce(ctx, providers, req, opts, routeModel, tried)
		if err == nil {
			return chunks, nil
		}
		lastErr = err
		wait, shouldRetry := m.shouldRetryAfterError(err, attempt, attempts, providers, routeModel, maxWait)
		if !shouldRetry {
			break
		}
		if errWait := waitForCooldown(ctx, wait); errWait != nil {
			return nil, errWait
		}
	}
	if lastErr != nil {
		return nil, lastErr
	}
	return nil, &Error{Code: "auth_not_found", Message: "no auth available"}
}

func (m *Manager) executeStreamPriorityOnce(ctx context.Context, providers []string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, routeModel string, tried map[string]struct{}) (<-chan cliproxyexecutor.StreamChunk, error) {
	candidates := m.collectPriorityCandidates(routeModel, providers, tried)
	if len(candidates) == 0 {
		return nil, &Error{Code: "auth_not_found", Message: "no auth available"}
	}

	// Reorder candidates to implement round-robin within the same priority level.
	candidates = m.applyPriorityRoundRobin(routeModel, candidates)

	var lastErr error
	for _, candidate := range candidates {
		auth := candidate.auth
		executor := candidate.executor

		entry := logEntryWithRequestID(ctx)
		debugLogPriorityAuthSelection(entry, auth, req.Model)

		tried[auth.ID] = struct{}{}
		execCtx := ctx
		if rt := m.roundTripperFor(auth); rt != nil {
			execCtx = context.WithValue(execCtx, roundTripperContextKey{}, rt)
			execCtx = context.WithValue(execCtx, "cliproxy.roundtripper", rt)
		}
		execReq := req
		execReq.Model, execReq.Metadata = rewriteModelForAuth(routeModel, req.Metadata, auth)
		execReq.Model, execReq.Metadata = m.applyOAuthModelMapping(auth, execReq.Model, execReq.Metadata)

		chunks, errStream := executor.ExecuteStream(execCtx, auth, execReq, opts)
		if errStream != nil {
			rerr := &Error{Message: errStream.Error()}
			var se cliproxyexecutor.StatusError
			if errors.As(errStream, &se) && se != nil {
				rerr.HTTPStatus = se.StatusCode()
			}
			result := Result{AuthID: auth.ID, Provider: auth.Provider, Model: routeModel, Success: false, Error: rerr}
			result.RetryAfter = retryAfterFromError(errStream)
			m.MarkResult(execCtx, result)
			lastErr = errStream
			continue
		}

		// Wrap the stream to mark result on completion
		out := make(chan cliproxyexecutor.StreamChunk)
		go func(streamCtx context.Context, streamAuth *Auth, streamChunks <-chan cliproxyexecutor.StreamChunk) {
			defer close(out)
			var failed bool
			for chunk := range streamChunks {
				if chunk.Err != nil && !failed {
					failed = true
					rerr := &Error{Message: chunk.Err.Error()}
					var se cliproxyexecutor.StatusError
					if errors.As(chunk.Err, &se) && se != nil {
						rerr.HTTPStatus = se.StatusCode()
					}
					m.MarkResult(streamCtx, Result{AuthID: streamAuth.ID, Provider: streamAuth.Provider, Model: routeModel, Success: false, Error: rerr})
				}
				out <- chunk
			}
			if !failed {
				m.MarkResult(streamCtx, Result{AuthID: streamAuth.ID, Provider: streamAuth.Provider, Model: routeModel, Success: true})
			}
		}(execCtx, auth.Clone(), chunks)
		return out, nil
	}

	if lastErr != nil {
		return nil, lastErr
	}
	return nil, &Error{Code: "auth_not_found", Message: "no auth available"}
}
