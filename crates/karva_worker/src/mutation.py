"""Replace one loader's source in memory and bypass original bytecode caches."""

import importlib.machinery
import os


def install(filename, source):
    """Compile and install one source override, returning import-use evidence."""
    target = os.path.normcase(os.path.realpath(filename))
    code = compile(source, filename, "exec", dont_inherit=True)
    loader_type = importlib.machinery.SourceFileLoader
    original_data = loader_type.get_data
    original_code = loader_type.get_code
    state = {"used": False}

    def matches(path):
        return os.path.normcase(os.path.realpath(path)) == target

    def get_data(self, path):
        if matches(path):
            state["used"] = True
            return source.encode("utf-8")
        return original_data(self, path)

    def get_code(self, fullname):
        if matches(self.path):
            state["used"] = True
            return code
        return original_code(self, fullname)

    loader_type.get_data = get_data
    loader_type.get_code = get_code
    return state
