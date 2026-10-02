package lease

import (
	"errors"
	"testing"
)

func TestFencedStoreRejectsStaleTokens(t *testing.T) {
	s := NewFencedStore()
	if err := s.Write(5, "k", "v5"); err != nil {
		t.Fatal(err)
	}
	if err := s.Write(3, "k", "v3"); err == nil {
		t.Fatal("stale token must be rejected")
	} else {
		var stale *ErrStaleToken
		if !errors.As(err, &stale) {
			t.Fatalf("expected ErrStaleToken, got %v", err)
		}
	}
	if err := s.Write(5, "k", "v5-again"); err == nil {
		t.Fatal("equal token must be rejected (not idempotent)")
	}
	if err := s.Write(6, "k", "v6"); err != nil {
		t.Fatal(err)
	}
	if v, _ := s.Read("k"); v != "v6" {
		t.Fatalf("value = %q, want v6", v)
	}
}
