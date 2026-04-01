"""Tests for Kryos standard library I/O module."""
import contextlib
import io as _io
import os
import shutil
import tempfile
import unittest

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter


class TestStdlibIO(unittest.TestCase):
    def _run(self, source: str) -> str:
        tokens = tokenize(source)
        module = parse(tokens)
        f = _io.StringIO()
        with contextlib.redirect_stdout(f):
            interp = Interpreter()
            interp.run(module)
        return f.getvalue().strip()

    @staticmethod
    def _kpath(path: str) -> str:
        """Escape a filesystem path for embedding in a Kryos string literal."""
        return path.replace("\\", "/")

    # ---- file_write + file_read ----

    def test_file_write_and_read(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            result = self._run(f'''
file_write("{kp}", "hello kryos")
println(file_read("{kp}"))
''')
            self.assertEqual(result, "hello kryos")
        finally:
            os.unlink(path)

    # ---- file_append ----

    def test_file_append(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            result = self._run(f'''
file_write("{kp}", "hello")
file_append("{kp}", " world")
println(file_read("{kp}"))
''')
            self.assertEqual(result, "hello world")
        finally:
            os.unlink(path)

    # ---- file_exists ----

    def test_file_exists(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            result = self._run(f'''
println(to_string(file_exists("{kp}")))
''')
            self.assertEqual(result, "true")
        finally:
            os.unlink(path)

    def test_file_exists_false(self):
        result = self._run('''
println(to_string(file_exists("/nonexistent_path_xyz_123")))
''')
        self.assertEqual(result, "false")

    # ---- file_delete ----

    def test_file_delete(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        result = self._run(f'''
println(to_string(file_delete("{kp}")))
println(to_string(file_exists("{kp}")))
''')
        lines = result.split("\n")
        self.assertEqual(lines[0], "true")
        self.assertEqual(lines[1], "false")

    # ---- file_lines ----

    def test_file_lines(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            with open(path, "w") as fh:
                fh.write("alpha\nbeta\n\ngamma\n")
            result = self._run(f'''
let lines = file_lines("{kp}")
println(to_string(len(lines)))
println(lines[0])
println(lines[1])
println(lines[2])
''')
            lines = result.split("\n")
            self.assertEqual(lines[0], "3")
            self.assertEqual(lines[1], "alpha")
            self.assertEqual(lines[2], "beta")
            self.assertEqual(lines[3], "gamma")
        finally:
            os.unlink(path)

    # ---- file_copy ----

    def test_file_copy(self):
        td = tempfile.mkdtemp()
        src = os.path.join(td, "src.txt")
        dst = os.path.join(td, "dst.txt")
        ksrc = self._kpath(src)
        kdst = self._kpath(dst)
        try:
            with open(src, "w") as fh:
                fh.write("copy me")
            result = self._run(f'''
file_copy("{ksrc}", "{kdst}")
println(file_read("{kdst}"))
''')
            self.assertEqual(result, "copy me")
        finally:
            shutil.rmtree(td)

    # ---- file_move ----

    def test_file_move(self):
        td = tempfile.mkdtemp()
        src = os.path.join(td, "orig.txt")
        dst = os.path.join(td, "moved.txt")
        ksrc = self._kpath(src)
        kdst = self._kpath(dst)
        try:
            with open(src, "w") as fh:
                fh.write("move me")
            result = self._run(f'''
file_move("{ksrc}", "{kdst}")
println(file_read("{kdst}"))
println(to_string(file_exists("{ksrc}")))
''')
            lines = result.split("\n")
            self.assertEqual(lines[0], "move me")
            self.assertEqual(lines[1], "false")
        finally:
            shutil.rmtree(td)

    # ---- file_size ----

    def test_file_size(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            with open(path, "w") as fh:
                fh.write("12345")
            result = self._run(f'''
println(to_string(file_size("{kp}")))
''')
            self.assertEqual(result, "5")
        finally:
            os.unlink(path)

    # ---- file_modified ----

    def test_file_modified(self):
        fd, path = tempfile.mkstemp(suffix=".txt")
        os.close(fd)
        kp = self._kpath(path)
        try:
            result = self._run(f'''
let ts = file_modified("{kp}")
println(to_string(ts > 0))
''')
            self.assertEqual(result, "true")
        finally:
            os.unlink(path)

    # ---- file_is_dir / file_is_file ----

    def test_file_is_dir_and_file(self):
        td = tempfile.mkdtemp()
        fd, fpath = tempfile.mkstemp(dir=td)
        os.close(fd)
        kd = self._kpath(td)
        kf = self._kpath(fpath)
        try:
            result = self._run(f'''
println(to_string(file_is_dir("{kd}")))
println(to_string(file_is_file("{kf}")))
println(to_string(file_is_dir("{kf}")))
println(to_string(file_is_file("{kd}")))
''')
            lines = result.split("\n")
            self.assertEqual(lines[0], "true")
            self.assertEqual(lines[1], "true")
            self.assertEqual(lines[2], "false")
            self.assertEqual(lines[3], "false")
        finally:
            shutil.rmtree(td)

    # ---- dir_create + dir_list ----

    def test_dir_create_and_list(self):
        td = tempfile.mkdtemp()
        sub = os.path.join(td, "a", "b")
        ksub = self._kpath(sub)
        try:
            # Create a nested dir, then create files in it
            result = self._run(f'''
dir_create("{ksub}")
file_write("{ksub}/x.txt", "x")
file_write("{ksub}/a.txt", "a")
let items = dir_list("{ksub}")
println(to_string(len(items)))
println(items[0])
println(items[1])
''')
            lines = result.split("\n")
            self.assertEqual(lines[0], "2")
            self.assertEqual(lines[1], "a.txt")
            self.assertEqual(lines[2], "x.txt")
        finally:
            shutil.rmtree(td)

    # ---- dir_remove ----

    def test_dir_remove(self):
        td = tempfile.mkdtemp()
        sub = os.path.join(td, "removeme")
        os.makedirs(sub)
        ksub = self._kpath(sub)
        try:
            with open(os.path.join(sub, "f.txt"), "w") as fh:
                fh.write("data")
            result = self._run(f'''
println(to_string(dir_remove("{ksub}")))
println(to_string(file_exists("{ksub}")))
''')
            lines = result.split("\n")
            self.assertEqual(lines[0], "true")
            self.assertEqual(lines[1], "false")
        finally:
            if os.path.exists(td):
                shutil.rmtree(td)

    # ---- glob ----

    def test_glob(self):
        td = tempfile.mkdtemp()
        kd = self._kpath(td)
        try:
            for name in ["a.kry", "b.kry", "c.txt"]:
                with open(os.path.join(td, name), "w") as fh:
                    fh.write("")
            result = self._run(f'''
let matches = glob("{kd}/*.kry")
println(to_string(len(matches)))
''')
            self.assertEqual(result, "2")
        finally:
            shutil.rmtree(td)

    # ---- path_join ----

    def test_path_join(self):
        result = self._run('''
println(path_join("a", "b", "c.txt"))
''')
        self.assertEqual(result, "a/b/c.txt")

    # ---- path_dirname ----

    def test_path_dirname(self):
        result = self._run('''
println(path_dirname("a/b/c.txt"))
''')
        self.assertEqual(result, "a/b")

    # ---- path_basename ----

    def test_path_basename(self):
        result = self._run('''
println(path_basename("a/b/c.txt"))
''')
        self.assertEqual(result, "c.txt")

    # ---- path_extension ----

    def test_path_extension(self):
        result = self._run('''
println(path_extension("file.kry"))
''')
        self.assertEqual(result, "kry")

    def test_path_extension_no_ext(self):
        result = self._run('''
println(path_extension("Makefile"))
''')
        self.assertEqual(result, "")

    # ---- path_resolve ----

    def test_path_resolve(self):
        result = self._run('''
let p = path_resolve(".")
println(to_string(len(p) > 0))
''')
        self.assertEqual(result, "true")

    # ---- env_get + env_set ----

    def test_env_get_and_set(self):
        key = "_KRYOS_TEST_ENV_VAR_XYZ"
        try:
            result = self._run(f'''
env_set("{key}", "kryos_val")
println(env_get("{key}"))
''')
            self.assertEqual(result, "kryos_val")
        finally:
            os.environ.pop(key, None)

    def test_env_get_missing(self):
        result = self._run('''
println(env_get("_KRYOS_NONEXISTENT_VAR_ABC"))
''')
        self.assertEqual(result, "")

    # ---- cwd ----

    def test_cwd(self):
        result = self._run('''
let d = cwd()
println(to_string(len(d) > 0))
''')
        self.assertEqual(result, "true")

    # ---- temp_file ----

    def test_temp_file(self):
        result = self._run('''
let p = temp_file()
println(to_string(file_exists(p)))
''')
        self.assertEqual(result, "true")

    # ---- temp_dir ----

    def test_temp_dir(self):
        result = self._run('''
let d = temp_dir()
println(to_string(file_is_dir(d)))
''')
        self.assertEqual(result, "true")


if __name__ == "__main__":
    unittest.main()
