'use strict';

const {createHash} = require('node:crypto');
const {readFileSync} = require('node:fs');
const {join} = require('node:path');
const writeOutput = process.stdout.write.bind(process.stdout);
process.stdout.write = process.stderr.write.bind(process.stderr);

async function main() {
  const version = name => JSON.parse(readFileSync(join(__dirname, 'node_modules', name, 'package.json'))).version;
  if (process.versions.node.split('.')[0] !== '24' ||
      version('@actions/attest') !== '3.2.0' ||
      version('@sigstore/core') !== '3.2.1') {
    throw new Error('Unsupported claim attestation tool version');
  }
  const {attest} = await import('@actions/attest');
  if (Object.hasOwn(process.env, 'GH_PAT') || Object.hasOwn(process.env, 'GH_TOKEN') ||
      !process.env.GITHUB_TOKEN ||
      process.env.GITHUB_SERVER_URL !== 'https://github.com' ||
      process.env.GITHUB_REF !== 'refs/heads/main' ||
      !process.env.ACTIONS_ID_TOKEN_REQUEST_URL ||
      !process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN) {
    throw new Error('Missing isolated GitHub job identity');
  }
  const entry = readFileSync(0);
  const result = await attest({
    subjects: [{name: 'entry.json', digest: {sha256: createHash('sha256').update(entry).digest('hex')}}],
    predicateType: 'https://github.com/tylerbutler/acme/attestations/claim-entry/v1',
    predicate: {},
    token: process.env.GITHUB_TOKEN,
    sigstore: 'public-good',
    skipWrite: true,
  });
  writeOutput(JSON.stringify(result.bundle));
}

main().catch(error => {
  process.stderr.write(error.message === 'Missing isolated GitHub job identity'
    ? 'Missing isolated GitHub job identity\n'
    : 'Claim attestation failed\n');
  process.exitCode = 1;
});
