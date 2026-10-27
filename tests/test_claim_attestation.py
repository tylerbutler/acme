import pathlib
import shutil
import subprocess
import unittest


class ClaimAttestationHelper(unittest.TestCase):
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
