// Checks a Tauri updater signature (minisign, base64-wrapped as Tauri writes it) against a
// public key and the release version, like the app does before installing an update (issue #168):
//
//   node scripts/verify-updater-signature.js <file> <file.sig> <pubkey-base64> <version>
//
// The release workflow runs it right after signing, so a private key that does not match the
// public key in the app fails the release instead of every user's update.
import { createHash, createPublicKey, verify } from "node:crypto";
import { Buffer } from "node:buffer";
import { createReadStream, readFileSync } from "node:fs";
import process from "node:process";

const [file, sigFile, pubkey, version] = process.argv.slice(2);
if (!file || !sigFile || !pubkey || !version) {
  console.error("usage: verify-updater-signature.js <file> <file.sig> <pubkey-base64> <version>");
  process.exit(2);
}

const unwrap = (text) => Buffer.from(text.trim(), "base64").toString("utf8").split("\n");
const fail = (message) => {
  console.error(`::error::${file}: ${message}`);
  process.exit(1);
};

const keyBytes = Buffer.from(unwrap(pubkey)[1] ?? "", "base64");
const [, sigLine = "", trustedLine = "", globalLine = ""] = unwrap(readFileSync(sigFile, "utf8"));
const sigBytes = Buffer.from(sigLine, "base64");
if (keyBytes.length !== 42 || sigBytes.length !== 74) fail("malformed key or signature");
if (!keyBytes.subarray(2, 10).equals(sigBytes.subarray(2, 10))) {
  fail("signed with another key than the public key in src-tauri/tauri.conf.json");
}

// Ed25519 public key as SubjectPublicKeyInfo: the fixed DER prefix plus the 32 key bytes.
const key = createPublicKey({
  key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), keyBytes.subarray(10)]),
  format: "der",
  type: "spki",
});

// "ED" signs the BLAKE2b-512 hash of the file (minisign's prehashed mode), "Ed" the file itself.
const algorithm = sigBytes.subarray(0, 2).toString("latin1");
let message;
if (algorithm === "ED") {
  const hash = createHash("blake2b512");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  message = hash.digest();
} else {
  message = readFileSync(file);
}
const signature = sigBytes.subarray(10);
if (!verify(null, message, key, signature)) fail("signature does not match the file");

const trusted = trustedLine.replace(/^trusted comment: /, "");
const global = Buffer.from(globalLine, "base64");
if (!verify(null, Buffer.concat([signature, Buffer.from(trusted, "utf8")]), key, global)) {
  fail("trusted comment was altered");
}
const signedVersion = trusted
  .split("\t")
  .find((field) => field.startsWith("version:"))
  ?.slice("version:".length);
if (signedVersion !== version)
  fail(`signed for version ${signedVersion ?? "none"}, not ${version}`);
console.log(`${file}: updater signature OK (version ${version})`);
