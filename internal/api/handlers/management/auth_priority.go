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
			name = strings.TrimSpace(auth.ID)
		}
		if name == "" {
			continue
		}

		// Only include file-based auth entries.
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
// Set priority to null to remove/reset the priority value.
// Set priority to 0 to explicitly set priority as 0.
func (h *Handler) PatchAuthPriority(c *gin.Context) {
	if h == nil || h.cfg == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "handler not initialized"})
		return
	}
	if h.authManager == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "core auth manager unavailable"})
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

	var targetID string
	configKey := name
	auths := h.authManager.List()
	for _, auth := range auths {
		if auth == nil {
			continue
		}
		authName := strings.TrimSpace(auth.FileName)
		if authName == "" {
			authName = strings.TrimSpace(auth.ID)
		}
		if authName == name || strings.TrimSpace(auth.ID) == name {
			targetID = auth.ID
			if fileName := strings.TrimSpace(auth.FileName); fileName != "" {
				configKey = fileName
			}
			break
		}
	}

	if targetID == "" {
		c.JSON(http.StatusNotFound, gin.H{"error": "auth file not found"})
		return
	}

	targetAuth, ok := h.authManager.GetByID(targetID)
	if !ok || targetAuth == nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "auth file not found"})
		return
	}
	if targetAuth.Attributes == nil {
		targetAuth.Attributes = make(map[string]string)
	}

	if body.Priority == nil {
		delete(targetAuth.Attributes, "priority")
	} else {
		targetAuth.Attributes["priority"] = strconv.Itoa(*body.Priority)
	}
	targetAuth.UpdatedAt = time.Now()
	_, _ = h.authManager.Update(context.Background(), targetAuth)

	if body.Priority == nil {
		if h.cfg.AuthPriority != nil {
			delete(h.cfg.AuthPriority, configKey)
			if len(h.cfg.AuthPriority) == 0 {
				h.cfg.AuthPriority = nil
			}
		}
	} else {
		if h.cfg.AuthPriority == nil {
			h.cfg.AuthPriority = make(map[string]int)
		}
		h.cfg.AuthPriority[configKey] = *body.Priority
	}

	h.persist(c)
}
