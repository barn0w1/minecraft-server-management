from __future__ import annotations

import io
import os
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path

import production_deploy as deploy


class ProductionDeployTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        for name in (
            "akamai-api-token",
            "r2-api-token",
            "remote-tls-private-key.pem",
            "agent-client-ca-private-key.pem",
        ):
            path = self.root / name
            path.write_text("secret\n", encoding="utf-8")
            path.chmod(0o600)
        runtime = self.root / "r2-runtime.env"
        runtime.write_text(
            "RESTIC_PASSWORD=correct-horse-battery-staple\n"
            "AWS_DEFAULT_REGION=auto\n",
            encoding="utf-8",
        )
        runtime.chmod(0o600)
        for name in (
            "remote-tls-fullchain.pem",
            "remote-tls-root-ca.pem",
            "agent-client-ca.pem",
            "authorized_keys",
        ):
            (self.root / name).write_text("public\n", encoding="utf-8")

    def write_config(self) -> Path:
        config = self.root / "production.toml"
        config.write_text(
            textwrap.dedent(
                f"""\
                [release]
                version = "v0.1.0"
                repository = "barn0w1/minecraft-server-management"
                target = "x86_64-unknown-linux-musl"
                checksums_sha256 = "{'a' * 64}"
                expected_commit = "{'b' * 40}"

                [service]
                public_address = "agent.example.test:443"
                server_name = "agent.example.test"
                trust_domain = "example.test"

                [akamai]
                scope = "production"
                region = "jp-tyo-3"
                image = "linode/debian13"
                instance_type = "g6-nanode-1"
                firewall_id = 42
                max_active_instances = 1
                max_instance_lifetime_seconds = 43200

                [r2]
                account_id = "{'c' * 32}"
                parent_access_key_id = "parent-key"
                bucket = "minecraft"
                temporary_credential_ttl_seconds = 46800

                [files]
                akamai_api_token = "akamai-api-token"
                r2_api_token = "r2-api-token"
                remote_tls_private_key = "remote-tls-private-key.pem"
                agent_client_ca_private_key = "agent-client-ca-private-key.pem"
                r2_runtime_environment = "r2-runtime.env"
                remote_tls_fullchain = "remote-tls-fullchain.pem"
                remote_tls_root_ca = "remote-tls-root-ca.pem"
                agent_client_ca_certificate = "agent-client-ca.pem"
                authorized_keys = "authorized_keys"

                [acceptance]
                repository = "s3:https://{'c' * 32}.r2.cloudflarestorage.com/minecraft/acceptance"
                host_port = 25565
                timeout_seconds = 3600
                """
            ),
            encoding="utf-8",
        )
        return config

    def test_valid_config_and_generated_live_boundary(self) -> None:
        config = deploy.load_config(self.write_config())
        deploy.validate_config(config)
        disabled = deploy.render_environment(config, "d" * 64, live=False)
        enabled = deploy.render_environment(config, "d" * 64, live=True)
        self.assertIn("MCSERVER_AKAMAI_LIVE_ENABLED=false\n", disabled)
        self.assertIn("MCSERVER_AKAMAI_REAP_ORPHANS_ON_START=false\n", disabled)
        self.assertIn("MCSERVER_AKAMAI_LIVE_ENABLED=true\n", enabled)
        self.assertIn(
            "mcserver-node-agent-v0.1.0-x86_64-unknown-linux-musl", enabled
        )

    def test_ping_response_accepts_successful_json(self) -> None:
        self.assertTrue(
            deploy.ping_response_is_ok(
                '{\n  "status": "ok",\n  "version": "0.1.0"\n}'
            )
        )
        self.assertFalse(
            deploy.ping_response_is_ok('{"status":"error","version":"0.1.0"}')
        )
        self.assertFalse(deploy.ping_response_is_ok("status=ok version=0.1.0"))

    def test_long_lived_r2_keys_are_rejected(self) -> None:
        runtime = self.root / "r2-runtime.env"
        runtime.write_text(
            "RESTIC_PASSWORD=password\n"
            "AWS_DEFAULT_REGION=auto\n"
            "AWS_ACCESS_KEY_ID=forbidden\n",
            encoding="utf-8",
        )
        runtime.chmod(0o600)
        config = deploy.load_config(self.write_config())
        with self.assertRaisesRegex(deploy.DeployError, "exactly"):
            deploy.validate_config(config)

    def test_insecure_secret_permissions_are_rejected(self) -> None:
        (self.root / "akamai-api-token").chmod(0o644)
        config = deploy.load_config(self.write_config())
        with self.assertRaisesRegex(deploy.DeployError, "must not be accessible"):
            deploy.validate_config(config)

    def test_unknown_configuration_key_is_rejected(self) -> None:
        config_path = self.write_config()
        content = config_path.read_text(encoding="utf-8")
        config_path.write_text(
            content.replace(
                'trust_domain = "example.test"',
                'trust_domain = "example.test"\ntrust_domian = "typo.test"',
            ),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(deploy.DeployError, "trust_domian"):
            deploy.load_config(config_path)

    def test_archive_traversal_is_rejected(self) -> None:
        archive = self.root / "bad.tar.gz"
        with tarfile.open(archive, "w:gz") as package:
            member = tarfile.TarInfo("../escape")
            payload = b"bad"
            member.size = len(payload)
            package.addfile(member, io.BytesIO(payload))
        destination = self.root / "output"
        destination.mkdir()
        with self.assertRaisesRegex(deploy.DeployError, "unsafe"):
            deploy.safe_extract(archive, destination)
        self.assertFalse((self.root / "escape").exists())

    def test_report_is_atomic_and_secret_free_by_construction(self) -> None:
        report_path = self.root / "report.json"
        report = deploy.Report(command="check", config="/tmp/production.toml")
        report.release = "v0.1.0"
        report.record("deployment inputs")
        report.succeed()
        report.write(report_path)
        payload = report_path.read_text(encoding="utf-8")
        self.assertIn('"outcome": "passed"', payload)
        self.assertNotIn("correct-horse-battery-staple", payload)
        self.assertEqual(os.stat(report_path).st_mode & 0o777, 0o640)


if __name__ == "__main__":
    unittest.main()
