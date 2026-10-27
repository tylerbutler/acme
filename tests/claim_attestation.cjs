'use strict';

const assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {readFileSync} = require('node:fs');
const {join} = require('node:path');
const vm = require('node:vm');

const source = readFileSync(join(__dirname, '../scripts/claim-attestation/sign.cjs'), 'utf8');

async function exercise(overrides = {}) {
  const entry = Buffer.from('{"unicode":"café", "value":1}\n', 'utf8');
  const outputs = [];
  let captured;
  const process = {
    env: {
      GITHUB_TOKEN: 'offline-job-token',
      GITHUB_SERVER_URL: 'https://github.com',
      GITHUB_REF: 'refs/heads/main',
      ACTIONS_ID_TOKEN_REQUEST_URL: 'https://identity.invalid',
      ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'offline-identity',
      ...overrides.env,
    },
    versions: {node: overrides.node || '24.0.0'},
    stdout: {write: value => outputs.push(value)},
    stderr: {write: () => {}},
  };
  globalThis.__claimAttest = async options => {
    captured = options;
    return {bundle: {signedExactEntry: true}};
  };
  const script = new vm.Script(source, {
    importModuleDynamically: () => import('data:text/javascript,export const attest = globalThis.__claimAttest'),
  });
  await script.runInNewContext({
    __dirname: join(__dirname, '../scripts/claim-attestation'),
    process,
    require: name => {
      if (name === 'node:fs') {
        return {readFileSync: path => {
          if (path === 0) return entry;
          if (path.includes('@actions/attest')) {
            return JSON.stringify({version: overrides.sdk || '3.2.0'});
          }
          return JSON.stringify({version: overrides.core || '3.2.1'});
        }};
      }
      if (['node:crypto', 'node:path'].includes(name)) return require(name);
      throw new Error('Unexpected helper import');
    },
  });
  delete globalThis.__claimAttest;
  return {entry, captured, outputs, process};
}

async function main() {
  const result = await exercise();
  assert.equal(result.process.exitCode, undefined);
  assert.deepEqual(JSON.parse(JSON.stringify(result.captured)), {
    subjects: [{name: 'entry.json', digest: {
      sha256: createHash('sha256').update(result.entry).digest('hex'),
    }}],
    predicateType: 'https://github.com/tylerbutler/acme/attestations/claim-entry/v1',
    predicate: {},
    token: 'offline-job-token',
    sigstore: 'public-good',
    skipWrite: true,
  });
  assert.deepEqual(result.outputs, ['{"signedExactEntry":true}']);
  for (const overrides of [
    {env: {GH_PAT: 'must-not-reach-helper'}},
    {env: {GH_TOKEN: 'must-not-reach-helper'}},
    {env: {GITHUB_REF: 'refs/pull/1/merge'}},
    {env: {GITHUB_TOKEN: ''}},
    {env: {ACTIONS_ID_TOKEN_REQUEST_TOKEN: ''}},
    {node: '22.0.0'},
    {sdk: '3.1.0'},
    {core: '2.0.0'},
  ]) {
    const failure = await exercise(overrides);
    assert.equal(failure.process.exitCode, 1);
    assert.equal(failure.captured, undefined);
    assert.deepEqual(failure.outputs, []);
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
