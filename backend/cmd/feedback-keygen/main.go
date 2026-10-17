// feedback-keygen prints an Ed25519 signing pair for the RC feedback endpoint.
//
// Usage:
//
//	go run ./cmd/feedback-keygen -key-id release-2026-10
//
// The seed goes into the shell build environment (MMA_FEEDBACK_SIGNING_SEED),
// the public key goes into the server environment (FEEDBACK_PUBLIC_KEYS).
// Never commit real values.
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/hex"
	"flag"
	"fmt"
	"log"
)

func main() {
	keyID := flag.String("key-id", "release", "key identifier embedded in the shell")
	flag.Parse()

	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		log.Fatalf("generate: %v", err)
	}
	seed := privateKey.Seed()
	fmt.Printf("key_id=%s\n", *keyID)
	fmt.Printf("seed_hex=%s\n", hex.EncodeToString(seed))
	fmt.Printf("public_key_hex=%s\n", hex.EncodeToString(publicKey))
	fmt.Println()
	fmt.Printf("shell build env:\n  MMA_FEEDBACK_SIGNING_SEED=%s\n  MMA_FEEDBACK_KEY_ID=%s\n",
		hex.EncodeToString(seed), *keyID)
	fmt.Printf("server env:\n  FEEDBACK_PUBLIC_KEYS=%s:%s\n", *keyID, hex.EncodeToString(publicKey))
}
