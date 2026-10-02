// Demo: multiple simulated clients (goroutines) contend for a lease on
// one resource. State and the audit log are persisted to local files.
// One client is paused past its lease expiry; after it wakes up, its
// stale fencing token is rejected by the downstream store.
package main

import (
	"flag"
	"fmt"
	"log"
	"time"

	"locallease/client"
	"locallease/lease"
)

func main() {
	stateFile := flag.String("state", "demo-state.json", "local state file")
	logFile := flag.String("log", "demo-log.jsonl", "local audit log file")
	flag.Parse()

	mgr, err := lease.NewManager(lease.RealClock{}, lease.NewFileStore(*stateFile), lease.NewLogger(*logFile))
	if err != nil {
		log.Fatal(err)
	}
	store := lease.NewFencedStore()
	const resource = "report-generator"
	ttl := 500 * time.Millisecond

	alice := client.New("alice", mgr)
	bob := client.New("bob", mgr)

	// Alice acquires the lease and writes once.
	la, ok, err := alice.Acquire(resource, ttl)
	must(err)
	fmt.Printf("alice acquire ok=%v token=%d\n", ok, la.Token)
	must(alice.Write(store, la, "report", "alice-v1"))

	// Alice pauses (GC / partition) longer than her lease.
	alice.Pause()
	time.Sleep(700 * time.Millisecond) // real time passes; lease expires

	// Bob can now take over with a higher fencing token.
	lb, ok, err := bob.Acquire(resource, ttl)
	must(err)
	fmt.Printf("bob acquire ok=%v token=%d\n", ok, lb.Token)
	must(bob.Write(store, lb, "report", "bob-v1"))

	// Alice wakes up: renew fails, and her stale token is rejected.
	alice.Resume()
	if _, ok, err := alice.Renew(la, ttl); err != nil || !ok {
		fmt.Printf("alice renew after pause: ok=%v err=%v (expected failure)\n", ok, err)
	}
	if err := alice.Write(store, la, "report", "alice-v2"); err != nil {
		fmt.Printf("alice stale write rejected: %v\n", err)
	}

	// Bob releases; Alice can acquire again with an even higher token.
	if ok, err := bob.Release(lb); err != nil || !ok {
		log.Fatalf("bob release: ok=%v err=%v", ok, err)
	}
	la2, ok, err := alice.Acquire(resource, ttl)
	must(err)
	fmt.Printf("alice re-acquire ok=%v token=%d (monotonic)\n", ok, la2.Token)

	v, _ := store.Read("report")
	fmt.Printf("final report value: %q\n", v)
	fmt.Printf("audit log (%d events) written to %s, state in %s\n",
		len(mgr.Events()), *logFile, *stateFile)
}

func must(err error) {
	if err != nil {
		log.Fatal(err)
	}
}
