package client

import (
	"errors"
	"testing"
	"time"

	"locallease/lease"
)

var t0 = time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)

func newEnv() (*lease.FakeClock, *lease.Manager, *lease.FencedStore) {
	clk := lease.NewFakeClock(t0)
	mgr, err := lease.NewManager(clk, lease.NewMemoryStore(), nil)
	if err != nil {
		panic(err)
	}
	return clk, mgr, lease.NewFencedStore()
}

// Full pause-and-recover scenario: alice holds the lease, gets paused
// past its expiry, bob takes over, and when alice wakes up both her
// renew and her fenced write are rejected.
func TestPausedClientLosesLeaseAndIsFencedOut(t *testing.T) {
	clk, mgr, store := newEnv()
	alice := New("alice", mgr)
	bob := New("bob", mgr)
	const ttl = 5 * time.Second

	la, ok, err := alice.Acquire("res", ttl)
	if err != nil || !ok {
		t.Fatalf("alice acquire: ok=%v err=%v", ok, err)
	}
	if err := alice.Write(store, la, "doc", "alice-1"); err != nil {
		t.Fatal(err)
	}

	// Alice freezes (GC pause / partition); time keeps flowing.
	alice.Pause()
	clk.Advance(10 * time.Second)

	// While paused, alice cannot do anything.
	if _, _, err := alice.Renew(la, ttl); !errors.Is(err, ErrPaused) {
		t.Fatalf("paused renew: err=%v, want ErrPaused", err)
	}

	// Bob takes over the expired lease with a higher token.
	lb, ok, err := bob.Acquire("res", ttl)
	if err != nil || !ok {
		t.Fatalf("bob acquire after expiry: ok=%v err=%v", ok, err)
	}
	if lb.Token <= la.Token {
		t.Fatalf("bob token %d must exceed alice token %d", lb.Token, la.Token)
	}
	if err := bob.Write(store, lb, "doc", "bob-1"); err != nil {
		t.Fatal(err)
	}

	// Alice wakes up: her lease is gone and her token is stale.
	alice.Resume()
	if _, ok, err := alice.Renew(la, ttl); err != nil || ok {
		t.Fatalf("alice renew after recovery must fail: ok=%v err=%v", ok, err)
	}
	if _, ok, err := alice.Acquire("res", ttl); err != nil || ok {
		t.Fatalf("alice acquire while bob holds lease must fail: ok=%v err=%v", ok, err)
	}
	var stale *lease.ErrStaleToken
	if err := alice.Write(store, la, "doc", "alice-2"); !errors.As(err, &stale) {
		t.Fatalf("alice stale write must be rejected with ErrStaleToken, got %v", err)
	}
	if v, _ := store.Read("doc"); v != "bob-1" {
		t.Fatalf("doc = %q, want bob-1 (stale write must not apply)", v)
	}
}

// A client that pauses briefly (lease still valid) can resume and renew.
func TestShortPauseKeepsLease(t *testing.T) {
	clk, mgr, _ := newEnv()
	c := New("carol", mgr)
	const ttl = 10 * time.Second

	l, ok, _ := c.Acquire("res", ttl)
	if !ok {
		t.Fatal("acquire failed")
	}
	c.Pause()
	clk.Advance(4 * time.Second) // still within ttl
	c.Resume()

	rl, ok, err := c.Renew(l, ttl)
	if err != nil || !ok {
		t.Fatalf("renew after short pause: ok=%v err=%v", ok, err)
	}
	if rl.Token != l.Token {
		t.Fatalf("renew must keep token %d, got %d", l.Token, rl.Token)
	}
}

// Many clients race; exactly one wins and only the winner can write.
func TestConcurrentClientsSingleWriter(t *testing.T) {
	_, mgr, store := newEnv()
	const n = 16

	type result struct {
		id    string
		lease lease.Lease
	}
	won := make(chan result, n)
	done := make(chan struct{})
	for i := 0; i < n; i++ {
		go func(id string) {
			c := New(id, mgr)
			if l, ok, _ := c.Acquire("res", time.Minute); ok {
				won <- result{id, l}
			}
			done <- struct{}{}
		}(string(rune('a' + i)))
	}
	for i := 0; i < n; i++ {
		<-done
	}
	close(won)

	var winners []result
	for r := range won {
		winners = append(winners, r)
	}
	if len(winners) != 1 {
		t.Fatalf("expected exactly one winner, got %d", len(winners))
	}
	w := winners[0]
	c := New(w.id, mgr)
	if err := c.Write(store, w.lease, "k", "v"); err != nil {
		t.Fatalf("winner write: %v", err)
	}
}

func TestWriteWithForeignLeaseRejected(t *testing.T) {
	_, mgr, store := newEnv()
	a := New("a", mgr)
	b := New("b", mgr)
	l, ok, _ := a.Acquire("res", time.Minute)
	if !ok {
		t.Fatal("acquire failed")
	}
	if err := b.Write(store, l, "k", "v"); err == nil {
		t.Fatal("write with someone else's lease must fail")
	}
}
