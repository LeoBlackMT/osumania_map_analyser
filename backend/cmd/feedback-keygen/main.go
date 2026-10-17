// feedback-keygen manages the Ed25519 key used to sign RC feedback envelopes.
//
// Generate a new pair:
//
//	go run ./cmd/feedback-keygen -key-id release-2026-10
//
// Derive the public key from an existing seed (env or -seed-hex):
//
//	MMA_FEEDBACK_SIGNING_SEED=<64 hex> go run ./cmd/feedback-keygen -derive
//
// Verify a saved envelope against a public key:
//
//	go run ./cmd/feedback-keygen -verify-envelope envelope.json -public-key <64 hex>
//
// The seed goes into the shell build environment (MMA_FEEDBACK_SIGNING_SEED),
// the public key goes into the server environment (FEEDBACK_PUBLIC_KEYS).
// Never commit real values: the seed is the private key.
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"os"
	"strings"

	"osumania-telemetry/internal/feedback"
)

func main() {
	keyID := flag.String("key-id", "release", "key identifier embedded in the shell")
	derive := flag.Bool("derive", false, "derive the pair from MMA_FEEDBACK_SIGNING_SEED instead of generating")
	seedHex := flag.String("seed-hex", "", "32-byte seed as 64 hex chars (implies -derive)")
	publicOnly := flag.Bool("public-only", false, "print only the public key hex")
	envelopePath := flag.String("verify-envelope", "", "verify a signed envelope JSON file")
	publicKeyHex := flag.String("public-key", "", "public key hex for -verify-envelope")
	flag.Parse()

	if *envelopePath != "" {
		verifyEnvelope(*envelopePath, *publicKeyHex)
		return
	}

	if *seedHex == "" {
		*seedHex = strings.TrimSpace(os.Getenv("MMA_FEEDBACK_SIGNING_SEED"))
	}
	if *seedHex != "" {
		*derive = true
	}

	var publicKey ed25519.PublicKey
	var seed []byte
	if *derive {
		raw, err := hex.DecodeString(strings.TrimSpace(*seedHex))
		if err != nil || len(raw) != ed25519.SeedSize {
			log.Fatalf("seed must be 64 hex chars (32 bytes)")
		}
		seed = raw
		publicKey = ed25519.NewKeyFromSeed(raw).Public().(ed25519.PublicKey)
	} else {
		pub, priv, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			log.Fatalf("generate: %v", err)
		}
		publicKey = pub
		seed = priv.Seed()
	}

	if *publicOnly {
		fmt.Println(hex.EncodeToString(publicKey))
		return
	}

	fmt.Printf("key_id=%s\n", *keyID)
	if !*derive {
		fmt.Printf("seed_hex=%s\n", hex.EncodeToString(seed))
	}
	fmt.Printf("public_key_hex=%s\n", hex.EncodeToString(publicKey))
	fmt.Println()
	if !*derive {
		fmt.Printf("shell build env:\n  MMA_FEEDBACK_SIGNING_SEED=%s\n  MMA_FEEDBACK_KEY_ID=%s\n",
			hex.EncodeToString(seed), *keyID)
	}
	fmt.Printf("server env:\n  FEEDBACK_PUBLIC_KEYS=%s:%s\n", *keyID, hex.EncodeToString(publicKey))
}

func verifyEnvelope(path, publicKeyHex string) {
	if publicKeyHex == "" {
		log.Fatalf("-public-key is required with -verify-envelope")
	}
	pub, err := hex.DecodeString(strings.TrimSpace(publicKeyHex))
	if err != nil || len(pub) != ed25519.PublicKeySize {
		log.Fatalf("public key must be 64 hex chars")
	}
	body, err := os.ReadFile(path)
	if err != nil {
		log.Fatalf("read envelope: %v", err)
	}
	var env feedback.Envelope
	if err := json.Unmarshal(body, &env); err != nil {
		log.Fatalf("parse envelope: %v", err)
	}
	sig, err := hex.DecodeString(env.Auth.Signature)
	if err != nil || len(sig) != ed25519.SignatureSize {
		fmt.Println("signature: INVALID (bad hex or length)")
		os.Exit(1)
	}
	message := feedback.CanonicalSignString(env.Payload)
	if !ed25519.Verify(ed25519.PublicKey(pub), []byte(message), sig) {
		fmt.Println("signature: INVALID")
		os.Exit(1)
	}
	fmt.Printf("signature: OK (key_id=%s, bid=%d, vote=%.1f)\n", env.Auth.KeyID, env.Payload.Bid, env.Payload.Vote)
}
