package lease

import (
	"path/filepath"
	"testing"
	"time"
)

// Restarting with a still-valid lease must preserve it: the holder can
// renew, and nobody else can acquire.
func TestRestartPreservesValidLease(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	clk := NewFakeClock(t0)

	m1, err := NewManager(clk, NewFileStore(path), nil)
	if err != nil {
		t.Fatal(err)
	}
	l, ok, _ := m1.Acquire("res", "a", time.Minute)
	if !ok {
		t.Fatal("acquire failed")
	}

	// Simulate restart: brand-new manager over the same file.
	clk.Advance(10 * time.Second)
	m2, err := NewManager(clk, NewFileStore(path), nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok, _ := m2.Acquire("res", "b", time.Minute); ok {
		t.Fatal("valid lease must survive restart and block others")
	}
	if _, ok, _ := m2.Renew("res", "a", l.Token, time.Minute); !ok {
		t.Fatal("holder must be able to renew after restart")
	}
}

// Restarting after a lease has expired must NOT resurrect it: a new
// holder can acquire immediately, and the old holder cannot renew.
func TestRestartDoesNotResurrectExpiredLease(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	clk := NewFakeClock(t0)

	m1, err := NewManager(clk, NewFileStore(path), nil)
	if err != nil {
		t.Fatal(err)
	}
	l, ok, _ := m1.Acquire("res", "a", 5*time.Second)
	if !ok {
		t.Fatal("acquire failed")
	}

	// Process is down while the lease expires.
	clk.Advance(time.Hour)
	m2, err := NewManager(clk, NewFileStore(path), nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok, _ := m2.Renew("res", "a", l.Token, 5*time.Second); ok {
		t.Fatal("expired lease must not be renewable after restart")
	}
	l2, ok, _ := m2.Acquire("res", "b", 5*time.Second)
	if !ok {
		t.Fatal("new holder must acquire after restart despite stale record")
	}
	if l2.Token <= l.Token {
		t.Fatalf("token %d after restart must exceed pre-restart token %d", l2.Token, l.Token)
	}
}

// The fencing token counter itself is persisted, so tokens remain
// strictly increasing across restarts even with no lease on record.
func TestTokenCounterSurvivesRestart(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	clk := NewFakeClock(t0)

	m1, _ := NewManager(clk, NewFileStore(path), nil)
	l1, ok, _ := m1.Acquire("res", "a", time.Second)
	if !ok {
		t.Fatal("acquire failed")
	}
	if ok, _ := m1.Release("res", "a", l1.Token); !ok {
		t.Fatal("release failed")
	}

	m2, _ := NewManager(clk, NewFileStore(path), nil)
	l2, ok, _ := m2.Acquire("res", "b", time.Second)
	if !ok {
		t.Fatal("acquire after restart failed")
	}
	if l2.Token <= l1.Token {
		t.Fatalf("token %d after restart must exceed %d", l2.Token, l1.Token)
	}
	if m2.LastToken() != l2.Token {
		t.Fatalf("LastToken = %d, want %d", m2.LastToken(), l2.Token)
	}
}

// Release must be persisted too: a restart must not bring back a
// released lease.
func TestRestartAfterRelease(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	clk := NewFakeClock(t0)

	m1, _ := NewManager(clk, NewFileStore(path), nil)
	l, ok, _ := m1.Acquire("res", "a", time.Hour)
	if !ok {
		t.Fatal("acquire failed")
	}
	if ok, _ := m1.Release("res", "a", l.Token); !ok {
		t.Fatal("release failed")
	}

	m2, _ := NewManager(clk, NewFileStore(path), nil)
	if _, ok, _ := m2.Acquire("res", "b", time.Hour); !ok {
		t.Fatal("released lease must stay released after restart")
	}
}
