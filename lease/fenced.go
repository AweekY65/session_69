package lease

import (
	"fmt"
	"sync"
)

// FencedStore simulates a downstream resource (e.g. a storage service)
// that honors fencing tokens: it only accepts writes carrying a token
// strictly greater than every token it has seen before. This is how an
// old lock holder that wakes up after losing its lease is detected and
// rejected.
type FencedStore struct {
	mu       sync.Mutex
	maxToken uint64
	data     map[string]string
}

func NewFencedStore() *FencedStore {
	return &FencedStore{data: map[string]string{}}
}

// ErrStaleToken is returned when a write carries a fencing token that is
// not greater than the highest token the store has already accepted.
type ErrStaleToken struct {
	Token    uint64
	MaxToken uint64
}

func (e *ErrStaleToken) Error() string {
	return fmt.Sprintf("stale fencing token %d (already processed token %d)", e.Token, e.MaxToken)
}

// Write applies the write only if token is strictly greater than any
// previously accepted token.
func (f *FencedStore) Write(token uint64, key, value string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if token <= f.maxToken {
		return &ErrStaleToken{Token: token, MaxToken: f.maxToken}
	}
	f.maxToken = token
	f.data[key] = value
	return nil
}

func (f *FencedStore) Read(key string) (string, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	v, ok := f.data[key]
	return v, ok
}
