package lease

import "time"

// Lease represents an exclusive, time-bounded grant on a resource.
type Lease struct {
	Resource   string    `json:"resource"`
	HolderID   string    `json:"holderId"`
	Token      uint64    `json:"token"`
	AcquiredAt time.Time `json:"acquiredAt"`
	ExpiresAt  time.Time `json:"expiresAt"`
}

// Expired reports whether the lease is no longer valid at the given time.
// A lease is valid on [AcquiredAt, ExpiresAt); at exactly ExpiresAt it is expired.
func (l Lease) Expired(now time.Time) bool {
	return !now.Before(l.ExpiresAt)
}
