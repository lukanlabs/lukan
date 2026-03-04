/**
 * End-to-end encryption for relay traffic.
 *
 * Uses Web Crypto API: X25519 ECDH + HKDF-SHA256 + AES-256-GCM.
 * No npm dependencies — pure browser APIs.
 */

const E2E_HKDF_SALT = new TextEncoder().encode("lukan-e2e-v1");
const E2E_HKDF_INFO = new TextEncoder().encode("aes-256-gcm");

/** Check if the browser supports X25519 (Chrome 113+, Firefox 130+, Safari 17.2+). */
export async function isE2ESupported(): Promise<boolean> {
  try {
    await crypto.subtle.generateKey({ name: "X25519" }, false, [
      "deriveBits",
    ]);
    return true;
  } catch {
    return false;
  }
}

/** An X25519 keypair for ECDH. */
export interface E2EKeyPair {
  privateKey: CryptoKey;
  publicKeyBytes: Uint8Array; // 32 bytes raw
}

/** Generate an X25519 keypair. */
export async function generateX25519KeyPair(): Promise<E2EKeyPair> {
  const kp = await crypto.subtle.generateKey({ name: "X25519" }, false, [
    "deriveBits",
  ]);
  const rawPub = await crypto.subtle.exportKey(
    "raw",
    (kp as CryptoKeyPair).publicKey,
  );
  return {
    privateKey: (kp as CryptoKeyPair).privateKey,
    publicKeyBytes: new Uint8Array(rawPub),
  };
}

/** Derive an AES-256-GCM key from our private key + their public key bytes. */
async function deriveAesKey(
  privateKey: CryptoKey,
  theirPkBytes: Uint8Array,
): Promise<CryptoKey> {
  // Import their raw public key
  const theirPk = await crypto.subtle.importKey(
    "raw",
    theirPkBytes.buffer as ArrayBuffer,
    { name: "X25519" },
    false,
    [],
  );

  // ECDH → shared secret bits
  const sharedBits = await crypto.subtle.deriveBits(
    { name: "X25519", public: theirPk },
    privateKey,
    256,
  );

  // Import shared secret as HKDF key material
  const hkdfKey = await crypto.subtle.importKey(
    "raw",
    sharedBits,
    "HKDF",
    false,
    ["deriveKey"],
  );

  // HKDF → AES-256-GCM key
  return crypto.subtle.deriveKey(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: E2E_HKDF_SALT,
      info: E2E_HKDF_INFO,
    },
    hkdfKey,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

/**
 * Compute a safety number from two public keys.
 * SHA256(sorted(pk_a, pk_b))[0..3] → 6 hex chars.
 * Must produce the same result as the Rust implementation.
 */
export async function computeSafetyNumber(
  pkA: Uint8Array,
  pkB: Uint8Array,
): Promise<string> {
  // Sort keys so both sides get the same result
  const aFirst = compareBytes(pkA, pkB) <= 0;
  const sorted = new Uint8Array(64);
  sorted.set(aFirst ? pkA : pkB, 0);
  sorted.set(aFirst ? pkB : pkA, 32);

  const hash = await crypto.subtle.digest("SHA-256", sorted);
  const bytes = new Uint8Array(hash);
  return (
    bytes[0].toString(16).padStart(2, "0") +
    bytes[1].toString(16).padStart(2, "0") +
    bytes[2].toString(16).padStart(2, "0")
  );
}

/** Compare two byte arrays lexicographically. */
function compareBytes(a: Uint8Array, b: Uint8Array): number {
  const len = Math.min(a.length, b.length);
  for (let i = 0; i < len; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return a.length - b.length;
}

/** An established E2E session. */
export class E2ESession {
  private aesKey: CryptoKey;
  private nonceCounter = 0;
  readonly safetyNumber: string;

  constructor(aesKey: CryptoKey, safetyNumber: string) {
    this.aesKey = aesKey;
    this.safetyNumber = safetyNumber;
  }

  /** Encrypt plaintext string into { n, d } (base64 nonce + ciphertext). */
  async encrypt(plaintext: string): Promise<{ n: string; d: string }> {
    const nonce = this.nextNonce();
    const encoded = new TextEncoder().encode(plaintext);
    const ciphertext = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv: nonce.buffer as ArrayBuffer },
      this.aesKey,
      encoded,
    );
    return {
      n: toBase64(nonce),
      d: toBase64(new Uint8Array(ciphertext)),
    };
  }

  /** Decrypt { n, d } back to plaintext string. */
  async decrypt(nonceB64: string, ciphertextB64: string): Promise<string> {
    const nonce = fromBase64(nonceB64);
    const ciphertext = fromBase64(ciphertextB64);
    const plaintext = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: nonce.buffer as ArrayBuffer },
      this.aesKey,
      ciphertext.buffer as ArrayBuffer,
    );
    return new TextDecoder().decode(plaintext);
  }

  /** Generate a 12-byte nonce from the internal counter. */
  private nextNonce(): Uint8Array {
    const nonce = new Uint8Array(12);
    const view = new DataView(nonce.buffer);
    // Write counter as big-endian u64 at offset 4 (matches Rust implementation)
    // DataView doesn't have setBigUint64 everywhere, so split into two u32s
    const hi = Math.floor(this.nonceCounter / 0x100000000);
    const lo = this.nonceCounter >>> 0;
    view.setUint32(4, hi, false);
    view.setUint32(8, lo, false);
    this.nonceCounter++;
    return nonce;
  }
}

/**
 * Perform the E2E handshake as the browser (initiator).
 *
 * 1. Generate keypair
 * 2. Send e2e_hello with our public key
 * 3. Wait for e2e_hello_ack with daemon's public key
 * 4. Derive shared AES key
 */
export async function performHandshake(
  sendWs: (msg: object) => void,
  waitForAck: () => Promise<{ pk: string; safety_number: string }>,
): Promise<E2ESession> {
  const kp = await generateX25519KeyPair();

  // Send our public key to daemon
  sendWs({ type: "e2e_hello", pk: toBase64(kp.publicKeyBytes) });

  // Wait for daemon's response
  const ack = await waitForAck();
  const daemonPkBytes = fromBase64(ack.pk);

  if (daemonPkBytes.length !== 32) {
    throw new Error(`Invalid daemon public key length: ${daemonPkBytes.length}`);
  }

  // Derive AES key
  const aesKey = await deriveAesKey(kp.privateKey, daemonPkBytes);

  // Compute and verify safety number
  const safetyNumber = await computeSafetyNumber(
    kp.publicKeyBytes,
    daemonPkBytes,
  );

  if (safetyNumber !== ack.safety_number) {
    throw new Error(
      `Safety number mismatch! Expected ${safetyNumber}, got ${ack.safety_number}. Possible MITM attack.`,
    );
  }

  console.log(`[E2E] Encryption established. Safety number: ${safetyNumber}`);
  return new E2ESession(aesKey, safetyNumber);
}

// ── Base64 helpers ────────────────────────────────────────────

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

function fromBase64(str: string): Uint8Array {
  const binary = atob(str);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
