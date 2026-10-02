package lease

import (
	"fmt"
	"sync"
	"time"
)

// Manager coordinates leases over resources. It is safe for concurrent
// use by many goroutines (simulated clients). All state transitions are
// serialized through a single mutex and persisted before being committed
// to memory, so a restart never resurrects a state that was not saved.
type Manager struct {
	mu     sync.Mutex
	clock  Clock
	store  Store
	logger *Logger
	state  State
}

// NewManager loads persisted state from the store. Expired leases are
// pruned lazily at operation time (checked against the injected clock),
// never "refreshed" on load, so a restart cannot resurrect a dead lease.
func NewManager(clock Clock, store Store, logger *Logger) (*Manager, error) {
	st, err := store.Load()
	if err != nil {
		return nil, err
	}
	if st.Leases == nil {
		st.Leases = map[string]Lease{}
	}
	if logger == nil {
		logger = NewLogger("")
	}
	return &Manager{clock: clock, store: store, logger: logger, state: st}, nil
}

// Acquire tries to obtain a lease on resource for holder with the given TTL.
// It succeeds only if no valid (unexpired) lease exists. Every successful
// acquisition — including takeovers of expired leases — mints a new,
// strictly increasing fencing token.
func (m *Manager) Acquire(resource, holder string, ttl time.Duration) (Lease, bool, error) {
	if ttl <= 0 {
		return Lease{}, false, fmt.Errorf("ttl must be positive")
	}
	m.mu.Lock()
	defer m.mu.Unlock()

	now := m.clock.Now()
	if cur, ok := m.state.Leases[resource]; ok && !cur.Expired(now) {
		m.log(Event{Time: now, Op: "acquire-denied", Resource: resource, Holder: holder, Token: cur.Token,
			Detail: fmt.Sprintf("held by %s until %s", cur.HolderID, cur.ExpiresAt.Format(time.RFC3339Nano))})
		return cur, false, nil
	}

	token := m.state.LastToken + 1
	lease := Lease{
		Resource:   resource,
		HolderID:   holder,
		Token:      token,
		AcquiredAt: now,
		ExpiresAt:  now.Add(ttl),
	}
	next := m.state
	next.Leases = cloneLeases(m.state.Leases)
	next.Leases[resource] = lease
	next.LastToken = token
	if err := m.store.Save(next); err != nil {
		return Lease{}, false, err
	}
	m.state = next
	m.log(Event{Time: now, Op: "acquire", Resource: resource, Holder: holder, Token: token,
		Detail: fmt.Sprintf("expires %s", lease.ExpiresAt.Format(time.RFC3339Nano))})
	return lease, true, nil
}

// Renew extends the lease held by holder with the given fencing token.
// It fails if the lease does not exist, belongs to another holder, the
// token does not match, or the lease has already expired — an expired
// lease can never be renewed.
func (m *Manager) Renew(resource, holder string, token uint64, ttl time.Duration) (Lease, bool, error) {
	if ttl <= 0 {
		return Lease{}, false, fmt.Errorf("ttl must be positive")
	}
	m.mu.Lock()
	defer m.mu.Unlock()

	now := m.clock.Now()
	cur, ok := m.state.Leases[resource]
	if !ok || cur.HolderID != holder || cur.Token != token {
		m.log(Event{Time: now, Op: "renew-denied", Resource: resource, Holder: holder, Token: token,
			Detail: "no matching lease"})
		return Lease{}, false, nil
	}
	if cur.Expired(now) {
		m.log(Event{Time: now, Op: "renew-denied", Resource: resource, Holder: holder, Token: token,
			Detail: "lease already expired"})
		return Lease{}, false, nil
	}

	cur.ExpiresAt = now.Add(ttl)
	next := m.state
	next.Leases = cloneLeases(m.state.Leases)
	next.Leases[resource] = cur
	if err := m.store.Save(next); err != nil {
		return Lease{}, false, err
	}
	m.state = next
	m.log(Event{Time: now, Op: "renew", Resource: resource, Holder: holder, Token: token,
		Detail: fmt.Sprintf("expires %s", cur.ExpiresAt.Format(time.RFC3339Nano))})
	return cur, true, nil
}

// Release drops the lease if and only if holder and token match the
// current lease. Releasing with a stale token is a no-op failure.
func (m *Manager) Release(resource, holder string, token uint64) (bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	now := m.clock.Now()
	cur, ok := m.state.Leases[resource]
	if !ok || cur.HolderID != holder || cur.Token != token {
		m.log(Event{Time: now, Op: "release-denied", Resource: resource, Holder: holder, Token: token,
			Detail: "no matching lease"})
		return false, nil
	}

	next := m.state
	next.Leases = cloneLeases(m.state.Leases)
	delete(next.Leases, resource)
	if err := m.store.Save(next); err != nil {
		return false, err
	}
	m.state = next
	m.log(Event{Time: now, Op: "release", Resource: resource, Holder: holder, Token: token})
	return true, nil
}

// Snapshot returns the current lease on resource, if any (possibly expired).
func (m *Manager) Snapshot(resource string) (Lease, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	l, ok := m.state.Leases[resource]
	return l, ok
}

// LastToken returns the highest fencing token issued so far.
func (m *Manager) LastToken() uint64 {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.state.LastToken
}

func (m *Manager) log(e Event) { m.logger.Add(e) }

func cloneLeases(in map[string]Lease) map[string]Lease {
	out := make(map[string]Lease, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}

// Events returns the audit log recorded by this manager.
func (m *Manager) Events() []Event { return m.logger.Events() }
