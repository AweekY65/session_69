package lease

import (
	"encoding/json"
	"sync"
	"time"
)

// Event is a single audit-log entry describing a lease operation.
type Event struct {
	Time     time.Time `json:"time"`
	Op       string    `json:"op"` // acquire, renew, release, expire-takeover
	Resource string    `json:"resource"`
	Holder   string    `json:"holder"`
	Token    uint64    `json:"token"`
	Detail   string    `json:"detail,omitempty"`
}

// Logger is an append-only audit log. Events are kept in memory and,
// optionally, appended as JSON lines to a local file.
type Logger struct {
	mu     sync.Mutex
	events []Event
	file   string
}

// NewLogger creates a Logger. If logFile is non-empty, events are also
// appended to that local file as JSON lines.
func NewLogger(logFile string) *Logger {
	return &Logger{file: logFile}
}

func (l *Logger) Add(e Event) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.events = append(l.events, e)
	if l.file != "" {
		if data, err := json.Marshal(e); err == nil {
			appendLine(l.file, data)
		}
	}
}

// Events returns a copy of all recorded events.
func (l *Logger) Events() []Event {
	l.mu.Lock()
	defer l.mu.Unlock()
	out := make([]Event, len(l.events))
	copy(out, l.events)
	return out
}
