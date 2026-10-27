import pathlib
import shutil
import subprocess
import unittest


class ClaimAttestationHelper(unittest.TestCase):
    def test_installed_esm_sdk_reaches_job_identity_rejection(self):
        node = shutil.which("node")
        if node is None:
            self.skipTest("Node.js is unavailable")
        root = pathlib.Path(__file__).resolve().parents[1]
        helper = root / "scripts" / "claim-attestation"
        if not (helper / "node_modules" / "@actions" / "attest").is_dir():
            self.skipTest("The pinned attestation SDK is not installed")
        imported = subprocess.run(
            [
                node,
                "--input-type=module",
                "-e",
                "import {attest} from '@actions/attest'; "
                "if (typeof attest !== 'function') process.exit(1);",
            ],
            cwd=helper,
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
            env={},
        )
        self.assertEqual(imported.returncode, 0, imported.stderr)
        result = subprocess.run(
            [node, str(helper / "sign.cjs")],
            input='{"entry":"offline"}\n',
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
            env={},
        )
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "Missing isolated GitHub job identity\n")

    def test_exact_bytes_and_job_identity_only(self):
        node = shutil.which("node")
        if node is None:
            self.skipTest("Node.js is unavailable")
        script = pathlib.Path(__file__).with_name("claim_attestation.cjs")
        result = subprocess.run(
            [node, "--experimental-vm-modules", str(script)],
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
            env={},
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
