#!/usr/bin/env python3
"""Real JVM lifecycle tests; run from repository root. No mocked builds or instrumentation."""
import json
import os
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
    def __init__(self, root):
        self.lines = []
        self.process = subprocess.Popen([str(BINARY), 'run', '--project', str(root)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
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
    def exercise(self, fixture):
        with tempfile.TemporaryDirectory(prefix='hotswap with spaces ') as tmp:
            root = Path(tmp) / fixture
            shutil.copytree(REPO / 'examples' / fixture, root, ignore=shutil.ignore_patterns('build', 'target', '.gradle', '.java-hotswap'))
            before = {p.relative_to(root): p.read_bytes() for p in root.rglob('*') if p.is_file() and p.suffix in ('.xml', '.gradle')}
            session = Session(root)
            try:
                original = session.wait(r'radius=0 pid=')
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
    def launch_and_shutdown(self, signal):
        with tempfile.TemporaryDirectory(prefix='hotswap dap ') as tmp:
            root = Path(tmp)
            shutil.copytree(REPO / 'examples/plain', root, dirs_exist_ok=True)
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
                send(2,'launch',{'cwd':str(root),'request':'launch','adapter':'Java HotSwap'})
                send(3,'configurationDone',{})
                deadline = time.monotonic()+30
                while time.monotonic()<deadline and not any('radius=0' in m.get('body',{}).get('output','') for m in messages): time.sleep(.05)
                self.assertTrue(any('radius=0' in m.get('body',{}).get('output','') for m in messages), messages)
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
