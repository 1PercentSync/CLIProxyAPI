package management

import (
	"context"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

// GetAuthPriority returns the priority for all auth files.
// GET /v0/management/auth-priority
// Returns: {"auth-priority": {"filename.json": priority, ...}}
func (h *Handler) GetAuthPriority(c *gin.Context) {
	if h == nil || h.authManager == nil {
		c.JSON(http.StatusOK, gin.H{"auth-priority": map[string]int{}})
		return
	}

	auths := h.authManager.List()
	result := make(map[string]int, len(auths))

	for _, auth := range auths {
		if auth == nil {
			continue
		}
		name := strings.TrimSpace(auth.FileName)
		if name == "" {
			name = auth.ID
		}
		if name == "" {
			continue
		}

		// Only include file-based auth entries (ending with .json)
		// Skip API key entries like "claude:apikey:xxx", "codex:apikey:xxx"
		if !strings.HasSuffix(strings.ToLower(name), ".json") {
			continue
		}

		priority := 0
		if auth.Attributes != nil {
			if raw := strings.TrimSpace(auth.Attributes["priority"]); raw != "" {
				if p, err := strconv.Atoi(raw); err == nil {
					priority = p
				}
			}
		}

		result[name] = priority
	}

	c.JSON(http.StatusOK, gin.H{"auth-priority": result})
}

// PatchAuthPriority updates the priority for a single auth file by name.
// PATCH /v0/management/auth-priority
// Body: {"name": "filename.json", "priority": 10}
// Set priority to null to remove the priority setting (reset to default).
// Set priority to 0 to explicitly set priority as 0.
func (h *Handler) PatchAuthPriority(c *gin.Context) {
	if h == nil || h.cfg == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "handler not initialized"})
		return
	}

	var body struct {
		Name     *string `json:"name"`
		Priority *int    `json:"priority"`
	}
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid body"})
		return
	}
	if body.Name == nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "name is required"})
		return
	}

	name := strings.TrimSpace(*body.Name)
	if name == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "name cannot be empty"})
		return
	}

	// Find, verify, and update auth in a single pass
	var targetAuth *struct {
		auth    interface{ UpdatedAt() time.Time }
		authPtr interface{}
		found   bool
	}

	if h.authManager != nil {
		auths := h.authManager.List()
		for _, auth := range auths {
			if auth == nil {
				continue
			}
			authName := strings.TrimSpace(auth.FileName)
			if authName == "" {
				authName = auth.ID
			}
			if authName == name || auth.ID == name {
				// Found the auth - update it
				if auth.Attributes == nil {
					auth.Attributes = make(map[string]string)
				}

				if body.Priority == nil {
					// null means delete/reset to default
					delete(auth.Attributes, "priority")
				} else {
					auth.Attributes["priority"] = strconv.Itoa(*body.Priority)
				}
				auth.UpdatedAt = time.Now()
				_, _ = h.authManager.Update(context.Background(), auth)
				targetAuth = &struct {
					auth    interface{ UpdatedAt() time.Time }
					authPtr interface{}
					found   bool
				}{found: true}
				break
			}
		}

		if targetAuth == nil || !targetAuth.found {
			c.JSON(http.StatusNotFound, gin.H{"error": "auth file not found"})
			return
		}
	}

	// Update config's AuthPriority map
	if h.cfg.AuthPriority == nil {
		h.cfg.AuthPriority = make(map[string]int)
	}

	if body.Priority == nil {
		// null means delete/reset to default
		delete(h.cfg.AuthPriority, name)
	} else {
		h.cfg.AuthPriority[name] = *body.Priority
	}

	// Persist to config.yaml
	h.persist(c)
}
