#!/usr/bin/env python3
"""Real JVM lifecycle tests; run from repository root. No mocked builds or instrumentation."""
import json
import os
import platform
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import threading
import time
import unittest

REPO = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('JAVA_HOTSWAP_BINARY', REPO / 'companion/target/debug/java-hotswap')).resolve()

class Session:
    def __init__(self, root, env=None):
        self.lines = []
        self.process = subprocess.Popen([str(BINARY), 'run', '--project', str(root)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
        self.thread = threading.Thread(target=self.read, daemon=True)
        self.thread.start()
    def read(self):
        for line in self.process.stdout:
            self.lines.append(line)
    def wait(self, pattern, start=0, timeout=180):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for line in self.lines[start:]:
                if re.search(pattern, line):
                    return line
            if self.process.poll() is not None:
                break
            time.sleep(.05)
        raise AssertionError(f'Missing {pattern!r}; exit={self.process.poll()}\n'+''.join(self.lines[-120:]))
    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try: self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill(); self.process.wait()
        self.thread.join(timeout=5)
        self.process.stdout.close()

class HotSwap(unittest.TestCase):
    def exercise(self, fixture, nested=False):
        with tempfile.TemporaryDirectory(prefix='hotswap with spaces ') as tmp:
            root = Path(tmp) / fixture
            destination = root / 'java' if nested else root
            shutil.copytree(REPO / 'examples' / fixture, destination, ignore=shutil.ignore_patterns('build', 'build.nosync', 'target', '.gradle', '.java-hotswap'))
            before = {p.relative_to(root): p.read_bytes() for p in root.rglob('*') if p.is_file() and p.suffix in ('.xml', '.gradle')}
            env = os.environ.copy()
            if fixture == 'nested-gradle':
                # A real wrapper must win over even an unusable global Gradle.
                env['JAVA_HOTSWAP_GRADLE'] = str(root / 'nonexistent-global-gradle')
            session = Session(root, env)
            try:
                original = session.wait(r'radius=0 pid=')
                if fixture == 'nested-gradle':
                    session.wait(r'deps=3\.3\.6.*gson="resolved" flags=true')
                    model = json.loads((root / '.java-hotswap/gradle.json').read_text())[0]
                    self.assertEqual(model['mainClass'], 'blockland.Main')
                    self.assertTrue(all('build.nosync' in p for p in model['classes']))
                    self.assertTrue(any('gson-2.13.2.jar' in p for p in model['classpath']))
                    self.assertTrue(any('lwjgl-3.3.6.jar' in p for p in model['classpath']))
                    self.assertIn('--enable-native-access=ALL-UNNAMED', model['jvmArgs'])
                    self.assertIn('-Dhotswap.run=true', model['jvmArgs'])
                    if platform.system() == 'Darwin':
                        self.assertIn('-XstartOnFirstThread', model['jvmArgs'])
                        # The macOS Java launcher consumes this flag before JVM
                        # startup, so RuntimeMXBean input arguments omit it.
                        pid = re.search(r'pid=(\d+)', original)[1]
                        command = subprocess.check_output(['ps', '-p', pid, '-o', 'command='], text=True)
                        self.assertIn('-XstartOnFirstThread', command)
                    self.assertTrue((root / 'java/build.nosync/classes/java/main/blockland/Main.class').is_file())
                    self.assertFalse((root / '.java-hotswap/classes').exists())
                    session.wait(r'BUILD TOOL .*java/gradlew')
                    self.assertIn('cwd=' + str((root / 'java').resolve()), ''.join(session.lines))
                pid = re.search(r'pid=(\d+)', original)[1]
                source = next(p for p in root.rglob('*.java') if 'return 0;' in p.read_text())
                text = source.read_text()
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 10;'))
                session.wait(r'RELOADED ', offset)
                session.wait(rf'radius=10 pid={pid}\b', offset)
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return syntax error;'))
                session.wait(r'COMPILE FAILED', offset)
                session.wait(rf'radius=10 pid={pid}\b', len(session.lines))
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 20;').replace('public class ', 'public class ', 1).replace('{', '{ public static int addedField = 1;', 1))
                session.wait(r'UNSUPPORTED batch', offset)
                session.wait(rf'radius=10 pid={pid}\b', len(session.lines))
                # Reverting the structural edit must recover without restart.
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 30;'))
                session.wait(rf'radius=30 pid={pid}\b', offset)
                duplicate = subprocess.run([str(BINARY), 'run', '--project', str(root)], capture_output=True, text=True, timeout=10)
                self.assertNotEqual(duplicate.returncode, 0)
                self.assertIn('already running', duplicate.stderr)
                subprocess.run([str(BINARY), 'clear', '--project', str(root)], check=True, timeout=10)
                session.wait(r'\x1b\[2J')
                subprocess.run([str(BINARY), 'stop', '--project', str(root)], check=True, timeout=10)
                self.assertEqual(session.process.wait(timeout=15), 0)
                with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)
                for path, data in before.items(): self.assertEqual((root / path).read_bytes(), data)
            finally: session.close()
    def test_plain(self): self.exercise('plain')
    def test_gradle(self): self.exercise('gradle')
    def test_gradle_multi(self): self.exercise('gradle-multi')
    def test_maven(self): self.exercise('maven')
    def test_maven_multi(self): self.exercise('maven-multi')
    def test_nested_gradle(self): self.exercise('nested-gradle')
    def test_nested_maven(self): self.exercise('maven', nested=True)

    def gradle_jdk_case(self, version, toolchain, runtime):
        def home(major):
            options = [os.environ.get(f'JAVA_HOTSWAP_TEST_JDK{major}', ''), os.environ.get('JAVA_HOME', ''),
                       f'/Library/Java/JavaVirtualMachines/temurin-{major}.jdk/Contents/Home']
            for candidate in options:
                if candidate and (Path(candidate) / 'release').is_file() and re.search(rf'JAVA_VERSION="{major}[.\"]', (Path(candidate) / 'release').read_text()): return candidate
            self.skipTest(f'requires Java {major}; set JAVA_HOTSWAP_TEST_JDK{major}')
        java25, java21 = home(25), home(21)
        with tempfile.TemporaryDirectory(prefix='hotswap Gradle JDK ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/nested-gradle', root, dirs_exist_ok=True,
                            ignore=shutil.ignore_patterns('build.nosync', '.gradle', '.java-hotswap'))
            wrapper = root / 'java/gradle/wrapper/gradle-wrapper.properties'
            wrapper.write_text(wrapper.read_text().replace('gradle-9.1.0-bin.zip', f'gradle-{version}-bin.zip'))
            build = root / 'java/build.gradle'
            build.write_text(build.read_text() + f'\njava {{ toolchain {{ languageVersion = JavaLanguageVersion.of({toolchain}) }} }}\n')
            before = {p:p.read_bytes() for p in [wrapper, build, root / 'java/gradlew']}
            env = os.environ.copy()
            env['JAVA_HOME'] = java25
            env['JAVA_HOTSWAP_JDK_PATHS'] = os.pathsep.join([java25, java21])
            env['JAVA_HOTSWAP_GRADLE'] = str(root / 'missing-global-gradle')
            session = Session(root, env)
            try:
                line = session.wait(r'radius=0 pid=')
                pid = re.search(r'pid=(\d+)', line)[1]
                session.wait(rf'GRADLE {version}(?:\.0)? runtime Java {runtime}\b')
                module = json.loads((root / '.java-hotswap/gradle.json').read_text())[0]
                self.assertEqual(Path(module['java']).resolve().parent.parent, Path(home(toolchain)).resolve())
                source = root / 'java/src/main/java/blockland/Main.java'
                offset = len(session.lines)
                source.write_text(source.read_text().replace('return 0;', 'return 10;'))
                session.wait(rf'radius=10 pid={pid}\b', offset)
                self.assertNotIn('Unsupported class file major version', ''.join(session.lines))
                for path, content in before.items(): self.assertEqual(path.read_bytes(), content)
            finally: session.close()

    def test_gradle810_with_java25_default(self): self.gradle_jdk_case('8.10', 21, 21)
    def test_gradle91_with_java25_default(self): self.gradle_jdk_case('9.1.0', 25, 25)
    def test_gradle91_runtime25_preserves_java21_toolchain(self): self.gradle_jdk_case('9.1.0', 21, 25)

    def test_gradle_preflight_errors_before_build_scripts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/nested-gradle', root, dirs_exist_ok=True,
                            ignore=shutil.ignore_patterns('build.nosync', '.gradle', '.java-hotswap'))
            props = root / 'java/gradle/wrapper/gradle-wrapper.properties'
            props.write_text(props.read_text().replace('gradle-9.1.0-bin.zip', 'unknown-distribution.zip'))
            result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('set gradleVersion', result.stdout + result.stderr)
            self.assertNotIn('> Task', result.stdout + result.stderr)
            self.assertFalse((root / 'java/build.nosync').exists())
            props.write_text(props.read_text().replace('unknown-distribution.zip', 'gradle-8.10-bin.zip'))
            daemon = root / 'java/gradle/gradle-daemon-jvm.properties'
            daemon.write_text('toolchainVersion=25\n')
            result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('requests Java 25', result.stdout + result.stderr)
            self.assertNotIn('> Task', result.stdout + result.stderr)
            self.assertEqual(daemon.read_text(), 'toolchainVersion=25\n')

    def test_gradle_incompatible_explicit_runtime(self):
        java25 = os.environ.get('JAVA_HOTSWAP_TEST_JDK25', os.environ.get('JAVA_HOME', ''))
        if not java25 or not (Path(java25) / 'release').is_file() or not re.search(r'JAVA_VERSION="25[.\"]', (Path(java25) / 'release').read_text()):
            self.skipTest('requires Java 25 default')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/nested-gradle', root, dirs_exist_ok=True,
                            ignore=shutil.ignore_patterns('build.nosync', '.gradle', '.java-hotswap'))
            wrapper = root / 'java/gradle/wrapper/gradle-wrapper.properties'
            wrapper.write_text(wrapper.read_text().replace('gradle-9.1.0-bin.zip', 'gradle-8.10-bin.zip'))
            for setting in ['gradleJavaHome', 'org.gradle.java.home']:
                with self.subTest(setting=setting):
                    if setting == 'gradleJavaHome':
                        (root / '.java-hotswap.json').write_text(json.dumps({setting:java25}))
                    else:
                        (root / '.java-hotswap.json').unlink()
                        (root / 'java/gradle.properties').write_text(setting+'='+java25.replace('\\', '\\\\')+'\n')
                    result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('requires a runtime JDK in Java 8–23', result.stdout + result.stderr)
                    self.assertNotIn('> Task', result.stdout + result.stderr)

    @unittest.skipIf(os.name == 'nt', 'custom Unix launcher fixture')
    def test_custom_gradle_wrapper_reports_are_verified(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'gradle/wrapper').mkdir(parents=True)
            (root / 'build.gradle').write_text("throw new GradleException('MUST_NOT_EXECUTE')")
            (root / 'gradle/wrapper/gradle-wrapper.properties').write_text('distributionUrl=https\\://private.example/custom.zip\n')
            (root / '.java-hotswap.json').write_text(json.dumps({'gradleVersion':'8.10'}))
            wrapper = root / 'gradlew'
            for reported, jvm, error in [('9.1.0', '21', 'Wrapper reported Gradle 9.1.0'),
                                          ('8.10', '25', 'wrapper launched Java 25')]:
                with self.subTest(reported=reported, jvm=jvm):
                    wrapper.write_text('#!/bin/sh\ncase " $* " in\n*" --version "*) printf "Gradle '+reported+'\\nLauncher JVM: '+jvm+'.0.1\\n";;\n*) touch build-script-executed;;\nesac\n')
                    wrapper.chmod(0o755)
                    result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error, result.stdout + result.stderr)
                    self.assertFalse((root / 'build-script-executed').exists())

    def test_multiple_builds_and_explicit_selection(self):
        with tempfile.TemporaryDirectory(prefix='hotswap multiple ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/gradle', root / 'game')
            shutil.copytree(REPO / 'examples/maven', root / 'tools')
            result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Multiple Java build projects', result.stderr)
            self.assertIn('game (gradle)', result.stderr)
            self.assertIn('tools (maven)', result.stderr)
            self.assertNotIn('BUILDING plain', result.stderr)
            (root / '.java-hotswap.json').write_text(json.dumps({'projectRoot':'game'}))
            session = Session(root)
            try:
                session.wait(r'radius=0 pid=')
                session.wait(r'SELECTED gradle project .*game')
            finally: session.close()

    def test_broken_nested_build_never_falls_back(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/gradle', root / 'java')
            (root / 'java/build.gradle').write_text("throw new GradleException('BROKEN_NESTED_BUILD')")
            result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=180)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('BROKEN_NESTED_BUILD', result.stdout + result.stderr)
            self.assertNotIn('BUILDING plain', result.stdout + result.stderr)
            self.assertFalse((root / '.java-hotswap/classes').exists())
    @unittest.skipUnless(os.environ.get('JAVA_HOTSWAP_TEST_LWJGL') == '1', 'requires active graphical desktop')
    def test_lwjgl(self): self.exercise('lwjgl')

    def test_snapshot_failure_recovery(self):
        with tempfile.TemporaryDirectory(prefix='hotswap failure ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/plain', root, dirs_exist_ok=True)
            session = Session(root)
            try:
                pid = re.search(r'pid=(\d+)', session.wait(r'radius=0 pid='))[1]
                # A file in place of the output directory produces a genuine OS I/O failure.
                blocked = root / '.java-hotswap/reloads'
                blocked.write_text('blocked')
                source = root / 'src/Main.java'
                text = source.read_text()
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 10;'))
                session.wait('RELOAD FAILED', offset)
                session.wait(rf'radius=0 pid={pid}\b', len(session.lines))
                blocked.unlink()
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 60;'))
                session.wait(rf'radius=60 pid={pid}\b', offset)
            finally: session.close()

    def test_entry_ambiguity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/plain', root, dirs_exist_ok=True)
            source = (root / 'src/Main.java').read_text()
            (root / 'src/Other.java').write_text(source.replace('class Main', 'class Other'))
            result = subprocess.run([str(BINARY), 'run', '--project', tmp], capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Found 2 entry points', result.stderr)

    def test_save_during_build_and_cancel(self):
        with tempfile.TemporaryDirectory(prefix='hotswap build ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/gradle', root, dirs_exist_ok=True)
            build = root / 'build.gradle'
            # Slow only saved-source rebuilds, after the normal initial compilation.
            build.write_text(build.read_text()+"\ntasks.named('compileJava') { doFirst { if (file('slow').exists()) { println 'SLOW_BUILD_STARTED'; Thread.sleep(1500) } } }\n")
            session = Session(root)
            try:
                pid = re.search(r'pid=(\d+)', session.wait(r'radius=0 pid='))[1]
                (root / 'slow').touch()
                source = root / 'src/main/java/demo/Main.java'
                text = source.read_text()
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 10;'))
                session.wait('SLOW_BUILD_STARTED', offset)
                source.write_text(text.replace('return 0;', 'return 40;'))
                session.wait(rf'radius=40 pid={pid}\b', offset)
                deadline = time.monotonic()+30
                while time.monotonic()<deadline and sum('COMPILED (' in l for l in session.lines[offset:])<2: time.sleep(.05)
                self.assertGreaterEqual(sum('COMPILED (' in l for l in session.lines[offset:]), 2)
                offset = len(session.lines)
                source.write_text(text.replace('return 0;', 'return 50;'))
                session.wait('SLOW_BUILD_STARTED', offset)
                subprocess.run([str(BINARY), 'stop', '--project', tmp], check=True, timeout=10)
                self.assertEqual(session.process.wait(timeout=15), 0)
                with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)
            finally: session.close()


class Dap(unittest.TestCase):
    def test_launch_and_disconnect(self): self.launch_and_shutdown(False)
    def test_signal_shutdown(self): self.launch_and_shutdown(True)
    def test_nested_gradle_launch_and_reload(self): self.launch_and_shutdown(False, 'nested-gradle')
    def test_multiple_projects_launch_selection(self): self.launch_and_shutdown(False, 'nested-gradle', selected=True)
    def launch_and_shutdown(self, signal, fixture='plain', selected=False):
        with tempfile.TemporaryDirectory(prefix='hotswap dap ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples' / fixture, root, dirs_exist_ok=True, ignore=shutil.ignore_patterns('build.nosync', '.gradle', '.java-hotswap'))
            if selected:
                (root / 'other').mkdir()
                (root / 'other/settings.gradle').write_text("throw new GradleException('UNSELECTED_BUILD_MUST_NOT_EXECUTE')")
            process = subprocess.Popen([str(BINARY), 'dap'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            messages = []
            def read():
                while True:
                    line = process.stdout.readline()
                    if not line: return
                    length = int(line.split(b':')[1]); self.assertEqual(process.stdout.readline(), b'\r\n')
                    messages.append(json.loads(process.stdout.read(length)))
            thread = threading.Thread(target=read, daemon=True); thread.start()
            def send(seq, command, args):
                body = json.dumps({'seq':seq,'type':'request','command':command,'arguments':args}).encode()
                process.stdin.write(f'Content-Length: {len(body)}\r\n\r\n'.encode()+body); process.stdin.flush()
            try:
                send(1,'initialize',{})
                launch = {'cwd':str(root),'request':'launch','adapter':'Java HotSwap'}
                if selected: launch['projectRoot'] = 'java'
                send(2,'launch',launch)
                send(3,'configurationDone',{})
                deadline = time.monotonic()+180
                while time.monotonic()<deadline and process.poll() is None and not any('radius=0' in m.get('body',{}).get('output','') for m in messages): time.sleep(.05)
                self.assertTrue(any('radius=0' in m.get('body',{}).get('output','') for m in messages), messages)
                if fixture == 'nested-gradle':
                    pid = next(re.search(r'pid=(\d+)', m['body']['output'])[1] for m in messages if 'radius=0' in m.get('body',{}).get('output',''))
                    source = root / 'java/src/main/java/blockland/Main.java'
                    source.write_text(source.read_text().replace('return 0;', 'return 10;'))
                    deadline = time.monotonic()+180
                    while time.monotonic()<deadline and process.poll() is None and not any(f'radius=10 pid={pid}' in m.get('body',{}).get('output','') for m in messages): time.sleep(.05)
                    self.assertTrue(any(f'radius=10 pid={pid}' in m.get('body',{}).get('output','') for m in messages), messages)
                if signal:
                    pid = next(re.search(r'pid=(\d+)', m['body']['output'])[1] for m in messages if 'radius=0' in m.get('body',{}).get('output',''))
                    process.terminate()
                else:
                    send(4,'setBreakpoints',{'breakpoints':[{'line':2}]})
                    send(5,'disconnect',{})
                self.assertEqual(process.wait(timeout=15), 0)
                thread.join(timeout=5)
                self.assertTrue(any(m.get('event')=='terminated' for m in messages))
                if signal:
                    with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)
                else:
                    self.assertTrue(any(m.get('request_seq')==4 and not m['body']['breakpoints'][0]['verified'] for m in messages))
                self.assertEqual(process.stderr.read(), b'')
            finally:
                if process.poll() is None: process.kill(); process.wait()
                process.stdin.close(); process.stdout.close(); process.stderr.close()

if __name__ == '__main__': unittest.main()
