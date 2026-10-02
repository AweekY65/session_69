// Package client simulates lock clients running as local goroutines.
// A client can be paused (simulating a GC stop-the-world pause, network
// partition or process suspension) and later resumed.
package client

import (
	"errors"
	"fmt"
	"sync"
	"time"

	"locallease/lease"
)

// ErrPaused is returned by every operation attempted while the client
// is paused (simulating a frozen process).
var ErrPaused = errors.New("client is paused")

// Client is a simulated lock client. All operations go to the shared
// in-process Manager, mimicking RPCs to a lock server.
type Client struct {
	ID  string
	mgr *lease.Manager

	mu     sync.Mutex
	paused bool
}

func New(id string, mgr *lease.Manager) *Client {
	return &Client{ID: id, mgr: mgr}
}

// Pause freezes the client: subsequent operations fail with ErrPaused
// until Resume is called. Time (the manager's clock) keeps flowing.
func (c *Client) Pause() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.paused = true
}

// Resume unfreezes the client.
func (c *Client) Resume() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.paused = false
}

func (c *Client) isPaused() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.paused
}

// Acquire tries to obtain the lease on resource.
func (c *Client) Acquire(resource string, ttl time.Duration) (lease.Lease, bool, error) {
	if c.isPaused() {
		return lease.Lease{}, false, ErrPaused
	}
	return c.mgr.Acquire(resource, c.ID, ttl)
}

// Renew extends a previously acquired lease.
func (c *Client) Renew(l lease.Lease, ttl time.Duration) (lease.Lease, bool, error) {
	if c.isPaused() {
		return lease.Lease{}, false, ErrPaused
	}
	return c.mgr.Renew(l.Resource, c.ID, l.Token, ttl)
}

// Release gives up a previously acquired lease.
func (c *Client) Release(l lease.Lease) (bool, error) {
	if c.isPaused() {
		return false, ErrPaused
	}
	return c.mgr.Release(l.Resource, c.ID, l.Token)
}

// Write performs a fenced write to a downstream resource using the
// lease's fencing token. A stale token is rejected by the store.
func (c *Client) Write(store *lease.FencedStore, l lease.Lease, key, value string) error {
	if c.isPaused() {
		return ErrPaused
	}
	if l.HolderID != c.ID {
		return fmt.Errorf("lease belongs to %q, not %q", l.HolderID, c.ID)
	}
	return store.Write(l.Token, key, value)
}
