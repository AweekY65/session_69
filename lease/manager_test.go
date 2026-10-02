package lease

import (
	"fmt"
	"sync"
	"testing"
	"time"
)

var t0 = time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)

func newTestManager(clk Clock) *Manager {
	m, err := NewManager(clk, NewMemoryStore(), nil)
	if err != nil {
		panic(err)
	}
	return m
}

func TestAcquireExclusive(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	if _, ok, err := m.Acquire("res", "a", time.Second); err != nil || !ok {
		t.Fatalf("first acquire: ok=%v err=%v", ok, err)
	}
	if _, ok, err := m.Acquire("res", "b", time.Second); err != nil || ok {
		t.Fatalf("second acquire while held must fail: ok=%v err=%v", ok, err)
	}
	// Same holder re-acquiring a live lease also fails: one lease per resource.
	if _, ok, err := m.Acquire("res", "a", time.Second); err != nil || ok {
		t.Fatalf("re-acquire by same holder must fail: ok=%v err=%v", ok, err)
	}
}

func TestConcurrentAcquireExactlyOneWinner(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	const n = 32
	var wg sync.WaitGroup
	wins := make(chan string, n)
	tokens := make(chan uint64, n)
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func(id string) {
			defer wg.Done()
			l, ok, err := m.Acquire("res", id, time.Minute)
			if err != nil {
				t.Errorf("acquire err: %v", err)
				return
			}
			if ok {
				wins <- id
				tokens <- l.Token
			}
		}(fmt.Sprintf("client-%d", i))
	}
	wg.Wait()
	close(wins)
	close(tokens)

	var winners []string
	for w := range wins {
		winners = append(winners, w)
	}
	if len(winners) != 1 {
		t.Fatalf("expected exactly 1 winner, got %d: %v", len(winners), winners)
	}
	for tok := range tokens {
		if tok != 1 {
			t.Fatalf("first token must be 1, got %d", tok)
		}
	}
}

func TestRenewExtendsExpiry(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	l, ok, _ := m.Acquire("res", "a", 10*time.Second)
	if !ok {
		t.Fatal("acquire failed")
	}
	clk.Advance(8 * time.Second)
	rl, ok, err := m.Renew("res", "a", l.Token, 10*time.Second)
	if err != nil || !ok {
		t.Fatalf("renew before expiry: ok=%v err=%v", ok, err)
	}
	want := t0.Add(18 * time.Second)
	if !rl.ExpiresAt.Equal(want) {
		t.Fatalf("expiry = %v, want %v", rl.ExpiresAt, want)
	}
	// Lease must still be valid at the old expiry.
	clk.Advance(5 * time.Second) // now = 13s > original 10s expiry
	if _, ok, _ := m.Acquire("res", "b", time.Second); ok {
		t.Fatal("b must not acquire: renewed lease still valid")
	}
}

func TestRenewWithWrongHolderOrTokenFails(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	l, ok, _ := m.Acquire("res", "a", time.Minute)
	if !ok {
		t.Fatal("acquire failed")
	}
	if _, ok, _ := m.Renew("res", "b", l.Token, time.Minute); ok {
		t.Fatal("renew by wrong holder must fail")
	}
	if _, ok, _ := m.Renew("res", "a", l.Token+99, time.Minute); ok {
		t.Fatal("renew with wrong token must fail")
	}
}

func TestExpiredLeaseCannotBeRenewed(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	l, ok, _ := m.Acquire("res", "a", 5*time.Second)
	if !ok {
		t.Fatal("acquire failed")
	}
	clk.Advance(5 * time.Second) // exactly at expiry: expired
	if _, ok, _ := m.Renew("res", "a", l.Token, 5*time.Second); ok {
		t.Fatal("renew at/after expiry must fail")
	}
	clk.Advance(time.Hour)
	if _, ok, _ := m.Renew("res", "a", l.Token, 5*time.Second); ok {
		t.Fatal("renew long after expiry must fail")
	}
}

func TestExpiredLeaseTakeover(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	la, ok, _ := m.Acquire("res", "a", 5*time.Second)
	if !ok {
		t.Fatal("acquire failed")
	}
	clk.Advance(6 * time.Second)
	lb, ok, _ := m.Acquire("res", "b", 5*time.Second)
	if !ok {
		t.Fatal("b must take over expired lease")
	}
	if lb.Token <= la.Token {
		t.Fatalf("takeover token %d must exceed old token %d", lb.Token, la.Token)
	}
	// Old holder's renew must now fail (lease belongs to b).
	if _, ok, _ := m.Renew("res", "a", la.Token, 5*time.Second); ok {
		t.Fatal("old holder renew after takeover must fail")
	}
}

func TestFencingTokenStrictlyIncreasing(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	var last uint64
	acquire := func(holder string) uint64 {
		l, ok, err := m.Acquire("res", holder, 5*time.Second)
		if err != nil || !ok {
			t.Fatalf("acquire by %s: ok=%v err=%v", holder, ok, err)
		}
		if l.Token <= last {
			t.Fatalf("token %d not strictly greater than previous %d", l.Token, last)
		}
		last = l.Token
		return l.Token
	}

	acquire("a")
	clk.Advance(10 * time.Second) // expire
	acquire("b")
	if ok, _ := m.Release("res", "b", last); !ok {
		t.Fatal("release failed")
	}
	acquire("c") // after release
	// Different resources share the same global token sequence.
	l, ok, _ := m.Acquire("other", "a", time.Second)
	if !ok || l.Token <= last {
		t.Fatalf("token on other resource = %d, want > %d", l.Token, last)
	}
}

func TestReleaseAndReacquire(t *testing.T) {
	clk := NewFakeClock(t0)
	m := newTestManager(clk)

	l, ok, _ := m.Acquire("res", "a", time.Minute)
	if !ok {
		t.Fatal("acquire failed")
	}
	// Wrong holder / token cannot release.
	if ok, _ := m.Release("res", "b", l.Token); ok {
		t.Fatal("release by wrong holder must fail")
	}
	if ok, _ := m.Release("res", "a", l.Token+1); ok {
		t.Fatal("release with wrong token must fail")
	}
	if ok, _ := m.Release("res", "a", l.Token); !ok {
		t.Fatal("release by holder must succeed")
	}
	// Immediately re-acquirable, no need to wait for expiry.
	l2, ok, _ := m.Acquire("res", "b", time.Minute)
	if !ok {
		t.Fatal("re-acquire after release failed")
	}
	if l2.Token != l.Token+1 {
		t.Fatalf("token after release = %d, want %d", l2.Token, l.Token+1)
	}
}

func TestReleaseNonexistentLease(t *testing.T) {
	m := newTestManager(NewFakeClock(t0))
	if ok, _ := m.Release("nope", "a", 1); ok {
		t.Fatal("release of nonexistent lease must fail")
	}
}

func TestInvalidTTLRejected(t *testing.T) {
	m := newTestManager(NewFakeClock(t0))
	if _, _, err := m.Acquire("res", "a", 0); err == nil {
		t.Fatal("acquire with zero ttl must error")
	}
	if _, _, err := m.Renew("res", "a", 1, -time.Second); err == nil {
		t.Fatal("renew with negative ttl must error")
	}
}
