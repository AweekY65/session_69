package lease

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sync"
)

// State is the full persistable state of the lease manager.
type State struct {
	Leases map[string]Lease `json:"leases"`
	// LastToken persists the fencing token counter so tokens stay
	// strictly increasing even across process restarts.
	LastToken uint64 `json:"lastToken"`
}

func emptyState() State {
	return State{Leases: map[string]Lease{}}
}

// Store persists manager state. Implementations must be safe for
// use by a single Manager (which serializes calls with its own mutex).
type Store interface {
	Load() (State, error)
	Save(State) error
}

// MemoryStore keeps state in memory only.
type MemoryStore struct {
	mu    sync.Mutex
	state State
}

func NewMemoryStore() *MemoryStore {
	return &MemoryStore{state: emptyState()}
}

func (s *MemoryStore) Load() (State, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return cloneState(s.state), nil
}

func (s *MemoryStore) Save(st State) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.state = cloneState(st)
	return nil
}

func cloneState(st State) State {
	out := State{Leases: make(map[string]Lease, len(st.Leases)), LastToken: st.LastToken}
	for k, v := range st.Leases {
		out.Leases[k] = v
	}
	return out
}

// FileStore persists state as JSON in a local file. Writes are atomic
// (write to temp file + rename) so a crash cannot corrupt the state.
type FileStore struct {
	path string
	mu   sync.Mutex
}

func NewFileStore(path string) *FileStore {
	return &FileStore{path: path}
}

func (s *FileStore) Load() (State, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := os.ReadFile(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return emptyState(), nil
	}
	if err != nil {
		return State{}, fmt.Errorf("read state file: %w", err)
	}
	var st State
	if err := json.Unmarshal(data, &st); err != nil {
		return State{}, fmt.Errorf("decode state file: %w", err)
	}
	if st.Leases == nil {
		st.Leases = map[string]Lease{}
	}
	return st, nil
}

func (s *FileStore) Save(st State) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	data, err := json.MarshalIndent(st, "", "  ")
	if err != nil {
		return fmt.Errorf("encode state: %w", err)
	}
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return fmt.Errorf("write temp state file: %w", err)
	}
	if err := os.Rename(tmp, s.path); err != nil {
		return fmt.Errorf("rename state file: %w", err)
	}
	return nil
}

// Path returns the underlying file path (used by tests and demos).
func (s *FileStore) Path() string { return s.path }
