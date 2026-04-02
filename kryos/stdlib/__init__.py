"""
Kryos Standard Library

Built-in modules that extend the Kryos runtime with essential functionality.
All functions registered here are available in every .kry program without imports.
"""

from kryos.stdlib.string_utils import register_string_builtins
from kryos.stdlib.math_ext import register_math_builtins
from kryos.stdlib.collections import register_collection_builtins
from kryos.stdlib.json_module import register_json_builtins
from kryos.stdlib.io_module import register_io_builtins
from kryos.stdlib.crypto_module import register_crypto_builtins
from kryos.stdlib.term_module import register_term_builtins
from kryos.stdlib.net_module import register_net_builtins
from kryos.stdlib.map_module import register_map_builtins
from kryos.stdlib.process_module import register_process_builtins
from kryos.stdlib.string_ext_module import register_string_ext_builtins
from kryos.stdlib.db_module import register_db_builtins
from kryos.stdlib.server_module import register_server_builtins
from kryos.stdlib.auth_module import register_auth_builtins
from kryos.stdlib.claude_module import register_claude_builtins
from kryos.stdlib.stripe_module import register_stripe_builtins
from kryos.stdlib.email_module import register_email_builtins
from kryos.stdlib.config_module import register_config_builtins
from kryos.stdlib.regex_module import register_regex_builtins
from kryos.stdlib.datetime_module import register_datetime_builtins
from kryos.stdlib.set_module import register_set_builtins


def register_all_stdlib(interpreter) -> None:
    """Register all standard library built-in functions with the interpreter."""
    register_string_builtins(interpreter)
    register_math_builtins(interpreter)
    register_collection_builtins(interpreter)
    register_json_builtins(interpreter)
    register_io_builtins(interpreter)
    register_crypto_builtins(interpreter)
    register_term_builtins(interpreter)
    register_net_builtins(interpreter)
    register_map_builtins(interpreter)
    register_process_builtins(interpreter)
    register_string_ext_builtins(interpreter)
    register_db_builtins(interpreter)
    register_server_builtins(interpreter)
    register_auth_builtins(interpreter)
    register_claude_builtins(interpreter)
    register_stripe_builtins(interpreter)
    register_email_builtins(interpreter)
    register_config_builtins(interpreter)
    register_regex_builtins(interpreter)
    register_datetime_builtins(interpreter)
    register_set_builtins(interpreter)
