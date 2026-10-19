"""Exercise installation without touching the host daemon or granting capabilities."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

INSTALLER = Path(__file__).with_name("install.sh")


class InstallTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.data = self.base / 'data with spaces % $'
        self.config = self.base / 'config'
        self.appdir = self.base / 'AppDir'
        self.tools = self.base / 'tools'
        self.tools.mkdir()
        for name, body in {
            'id': 'echo 1000',
            'pkexec': 'exec "$@"',
            'setcap': 'exit "${FAIL_CAPS:-0}"',
            'getcap': 'echo "$1 cap_net_admin,cap_net_raw=eip"',
            'ldd': 'echo "libdbus-1.so.3 => /usr/lib/libdbus-1.so.3"',
            'systemctl': 'printf "%s\\n" "$*" >> "$SYSTEMCTL_LOG"',
        }.items():
            file = self.tools / name
            file.write_text('#!/bin/sh\n' + body + '\n')
            file.chmod(0o755)
        for name in ('airpods-daemon', 'airpods-cli'):
            file = self.appdir / 'usr/bin' / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('#!/bin/sh\nexit 99\n')
        icon = self.appdir / 'usr/share/icons/hicolor/scalable/apps/org.costa.AirPodsHelper.svg'
        icon.parent.mkdir(parents=True)
        icon.write_text('<svg/>')
        version = self.appdir / 'usr/share/airpods-helper/version'
        version.parent.mkdir(parents=True)
        version.write_text('0.3.0\n')
        self.image = self.base / 'download.AppImage'
        self.image.write_bytes(b'AppImage fixture')
        self.env = dict(os.environ, PATH=str(self.tools) + ':' + os.environ['PATH'],
                        HOME=str(self.base), XDG_DATA_HOME=str(self.data),
                        XDG_CONFIG_HOME=str(self.config), APPDIR=str(self.appdir),
                        APPIMAGE=str(self.image), SYSTEMCTL_LOG=str(self.base / 'systemctl.log'))

    def install(self, **env):
        return subprocess.run(['bash', str(INSTALLER)], env=dict(self.env, **env),
                              capture_output=True, text=True)

    def test_apprun_launches_tauri_with_arguments_and_gtk_hooks(self):
        gui = self.appdir / 'usr/bin/airpods-app'
        gui.write_text('#!/bin/sh\nprintf "%s\\n" "$PWD" "$GTK_HOOK_TEST" "$@"\n')
        gui.chmod(0o755)
        hooks = self.appdir / 'apprun-hooks'
        hooks.mkdir()
        (hooks / 'gtk.sh').write_text('export GTK_HOOK_TEST=loaded\n')
        result = subprocess.run([str(INSTALLER.with_name('AppRun')), '--minimized', 'with spaces'],
                                env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [str(self.appdir / 'usr'), 'loaded', '--minimized', 'with spaces'])

    def test_apprun_installer_uses_host_library_environment(self):
        setup = self.appdir / 'usr/share/airpods-helper/install.sh'
        setup.write_text('#!/bin/sh\nprintf "%s" "${LD_LIBRARY_PATH-unset}"\n')
        result = subprocess.run([str(INSTALLER.with_name('AppRun')), '--install'],
                                env=dict(self.env, LD_LIBRARY_PATH='/host/libraries'),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'unset')

    def test_install_and_update_preserve_settings(self):
        config = self.config / 'airpods-helper/config.toml'
        config.parent.mkdir(parents=True)
        config.write_text('auto_reconnect = false\n')
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        current = self.data / 'airpods-helper/current'
        previous = current.resolve()
        self.assertEqual((current / 'AirPods_Helper.AppImage').read_bytes(), self.image.read_bytes())
        self.assertEqual((current / 'appimage-version').read_text(), '0.3.0\n')
        service = (self.config / 'systemd/user/airpods-daemon.service').read_text()
        self.assertIn('data with spaces %% $$', service)
        desktop = (self.data / 'applications/org.costa.AirPodsHelper.desktop').read_text()
        self.assertIn('data with spaces %% \\$', desktop)
        (self.appdir / 'usr/share/airpods-helper/version').write_text('0.3.1\n')
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotEqual(previous, current.resolve())
        self.assertTrue(previous.is_dir())
        self.assertEqual((current / 'appimage-version').read_text(), '0.3.1\n')
        self.assertEqual(config.read_text(), 'auto_reconnect = false\n')
        self.assertIn('--user restart airpods-daemon.service', (self.base / 'systemctl.log').read_text())

    def test_capability_failure_keeps_previous_install(self):
        self.assertEqual(self.install().returncode, 0)
        current = self.data / 'airpods-helper/current'
        previous = current.resolve()
        result = self.install(FAIL_CAPS='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(current.resolve(), previous)
        self.assertEqual(list(previous.parent.iterdir()), [previous])

    def test_refuses_unmanaged_service(self):
        service = self.config / 'systemd/user/airpods-daemon.service'
        service.parent.mkdir(parents=True)
        service.write_text('existing service\n')
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('unmanaged file', result.stderr)
        self.assertEqual(service.read_text(), 'existing service\n')
        self.assertFalse((self.data / 'airpods-helper/current').exists())


if __name__ == '__main__':
    unittest.main()
