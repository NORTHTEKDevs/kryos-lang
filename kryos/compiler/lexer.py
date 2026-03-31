"""
Kryos Language - Lexer
Tokenizes .kry source files into a stream of Token objects.
"""

from __future__ import annotations

from typing import Iterator

from .tokens import (
    Token,
    TokenType,
    KEYWORDS,
    ATTRIBUTE_KEYWORDS,
    BUILTIN_TYPES,
)


class LexerError(Exception):
    """Raised when the lexer encounters an unrecoverable error."""

    def __init__(self, message: str, file: str, line: int, column: int) -> None:
        self.file = file
        self.line = line
        self.column = column
        super().__init__(f"{file}:{line}:{column}: {message}")


class Lexer:
    """
    Tokenizer for the Kryos programming language.

    Usage::

        lexer = Lexer(source_text, filename="main.kry")
        tokens = lexer.tokenize()          # list[Token]
        # or iterate lazily:
        for tok in lexer:
            ...
    """

    def __init__(self, source: str, filename: str = "<stdin>") -> None:
        self._src = source
        self._file = filename
        self._pos = 0          # byte/char offset into source
        self._line = 1
        self._col = 1
        self._tokens: list[Token] = []

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def tokenize(self) -> list[Token]:
        """Tokenize the entire source and return a list of tokens."""
        self._pos = 0
        self._line = 1
        self._col = 1
        self._tokens = []

        while not self._at_end():
            self._scan_token()

        self._emit(TokenType.EOF, "")
        return list(self._tokens)

    def __iter__(self) -> Iterator[Token]:
        return iter(self.tokenize())

    # ------------------------------------------------------------------
    # Helpers – character access
    # ------------------------------------------------------------------

    def _at_end(self) -> bool:
        return self._pos >= len(self._src)

    def _peek(self, offset: int = 0) -> str:
        idx = self._pos + offset
        if idx >= len(self._src):
            return "\0"
        return self._src[idx]

    def _advance(self) -> str:
        ch = self._src[self._pos]
        self._pos += 1
        if ch == "\n":
            self._line += 1
            self._col = 1
        else:
            self._col += 1
        return ch

    def _match(self, expected: str) -> bool:
        if self._at_end() or self._src[self._pos] != expected:
            return False
        self._advance()
        return True

    # ------------------------------------------------------------------
    # Token creation
    # ------------------------------------------------------------------

    def _emit(self, ttype: TokenType, value: str, line: int | None = None, col: int | None = None) -> None:
        self._tokens.append(
            Token(
                type=ttype,
                value=value,
                line=line if line is not None else self._line,
                column=col if col is not None else self._col,
                file=self._file,
            )
        )

    def _error_token(self, message: str, line: int, col: int) -> None:
        self._emit(TokenType.ERROR, message, line, col)

    def _error(self, message: str) -> LexerError:
        return LexerError(message, self._file, self._line, self._col)

    # ------------------------------------------------------------------
    # Whitespace & comments
    # ------------------------------------------------------------------

    def _skip_whitespace_and_comments(self) -> None:
        while not self._at_end():
            ch = self._peek()
            if ch in (" ", "\t", "\r", "\n"):
                self._advance()
            elif ch == "/" and self._peek(1) == "/":
                self._skip_line_comment()
            elif ch == "/" and self._peek(1) == "*":
                self._skip_block_comment()
            else:
                break

    def _skip_line_comment(self) -> None:
        # consume the //
        self._advance()
        self._advance()
        while not self._at_end() and self._peek() != "\n":
            self._advance()

    def _skip_block_comment(self) -> None:
        start_line = self._line
        start_col = self._col
        # consume /*
        self._advance()
        self._advance()
        depth = 1
        while not self._at_end() and depth > 0:
            if self._peek() == "/" and self._peek(1) == "*":
                self._advance()
                self._advance()
                depth += 1
            elif self._peek() == "*" and self._peek(1) == "/":
                self._advance()
                self._advance()
                depth -= 1
            else:
                self._advance()
        if depth != 0:
            raise self._error(f"unterminated block comment started at {start_line}:{start_col}")

    # ------------------------------------------------------------------
    # Main dispatcher
    # ------------------------------------------------------------------

    def _scan_token(self) -> None:
        self._skip_whitespace_and_comments()
        if self._at_end():
            return

        start_line = self._line
        start_col = self._col
        ch = self._peek()

        # --- Numbers ---
        if ch.isdigit():
            self._scan_number(start_line, start_col)
            return

        # --- Identifiers / keywords ---
        if ch.isalpha() or ch == "_":
            self._scan_identifier(start_line, start_col)
            return

        # --- Strings ---
        if ch == '"':
            self._scan_string(start_line, start_col)
            return

        # --- Chars ---
        if ch == "'":
            self._scan_char(start_line, start_col)
            return

        # --- Operators / punctuation ---
        self._scan_operator(start_line, start_col)

    # ------------------------------------------------------------------
    # Numbers
    # ------------------------------------------------------------------

    def _scan_number(self, sl: int, sc: int) -> None:
        start = self._pos

        if self._peek() == "0" and self._peek(1) in ("x", "X"):
            self._advance()  # 0
            self._advance()  # x
            if not self._is_hex(self._peek()):
                self._error_token("expected hex digit after '0x'", sl, sc)
                return
            while not self._at_end() and (self._is_hex(self._peek()) or self._peek() == "_"):
                self._advance()
            self._emit(TokenType.INTEGER, self._src[start:self._pos], sl, sc)
            return

        if self._peek() == "0" and self._peek(1) in ("b", "B"):
            self._advance()
            self._advance()
            if self._peek() not in ("0", "1"):
                self._error_token("expected binary digit after '0b'", sl, sc)
                return
            while not self._at_end() and self._peek() in ("0", "1", "_"):
                self._advance()
            self._emit(TokenType.INTEGER, self._src[start:self._pos], sl, sc)
            return

        if self._peek() == "0" and self._peek(1) in ("o", "O"):
            self._advance()
            self._advance()
            if not ("0" <= self._peek() <= "7"):
                self._error_token("expected octal digit after '0o'", sl, sc)
                return
            while not self._at_end() and ("0" <= self._peek() <= "7" or self._peek() == "_"):
                self._advance()
            self._emit(TokenType.INTEGER, self._src[start:self._pos], sl, sc)
            return

        # Decimal integer or float
        while not self._at_end() and (self._peek().isdigit() or self._peek() == "_"):
            self._advance()

        is_float = False
        # Fractional part: require digit after dot (avoid matching `..`)
        if self._peek() == "." and self._peek(1) != "." and self._peek(1).isdigit():
            is_float = True
            self._advance()  # .
            while not self._at_end() and (self._peek().isdigit() or self._peek() == "_"):
                self._advance()

        # Exponent
        if self._peek() in ("e", "E"):
            is_float = True
            self._advance()
            if self._peek() in ("+", "-"):
                self._advance()
            if not self._peek().isdigit():
                self._error_token("expected digit in exponent", sl, sc)
                return
            while not self._at_end() and (self._peek().isdigit() or self._peek() == "_"):
                self._advance()

        ttype = TokenType.FLOAT if is_float else TokenType.INTEGER
        self._emit(ttype, self._src[start:self._pos], sl, sc)

    @staticmethod
    def _is_hex(ch: str) -> bool:
        return ch.isdigit() or ch in "abcdefABCDEF"

    # ------------------------------------------------------------------
    # Identifiers & keywords
    # ------------------------------------------------------------------

    def _scan_identifier(self, sl: int, sc: int) -> None:
        start = self._pos
        while not self._at_end() and (self._peek().isalnum() or self._peek() == "_"):
            self._advance()
        text = self._src[start:self._pos]

        if text in KEYWORDS:
            self._emit(KEYWORDS[text], text, sl, sc)
        elif text in BUILTIN_TYPES:
            self._emit(TokenType.TYPE_IDENT, text, sl, sc)
        else:
            self._emit(TokenType.IDENTIFIER, text, sl, sc)

    # ------------------------------------------------------------------
    # Strings (with interpolation support)
    # ------------------------------------------------------------------

    def _scan_string(self, sl: int, sc: int) -> None:
        # Check for triple-quoted multiline string
        if self._peek(1) == '"' and self._peek(2) == '"':
            self._scan_triple_string(sl, sc)
            return

        self._advance()  # opening "

        buf: list[str] = []
        has_interp = False

        while not self._at_end() and self._peek() != '"':
            if self._peek() == "\n":
                raise self._error("unterminated string literal")

            if self._peek() == "\\":
                buf.append(self._scan_escape())
            elif self._peek() == "{":
                has_interp = True
                # Emit accumulated text as STRING_PART
                self._emit(TokenType.STRING_PART, "".join(buf), sl, sc)
                buf = []
                interp_sl, interp_sc = self._line, self._col
                self._advance()  # {
                self._emit(TokenType.INTERP_START, "{", interp_sl, interp_sc)
                # Lex tokens until matching }
                self._scan_interpolation_body()
                if self._at_end() or self._peek() != "}":
                    raise self._error("unterminated string interpolation; expected '}'")
                close_sl, close_sc = self._line, self._col
                self._advance()  # }
                self._emit(TokenType.INTERP_END, "}", close_sl, close_sc)
                # Update start location for the next string part
                sl, sc = self._line, self._col
            else:
                buf.append(self._advance())

        if self._at_end():
            raise self._error("unterminated string literal")

        self._advance()  # closing "

        if has_interp:
            # Emit remaining tail as STRING_PART (even if empty, parser can filter)
            if buf:
                self._emit(TokenType.STRING_PART, "".join(buf), sl, sc)
        else:
            self._emit(TokenType.STRING, "".join(buf), sl, sc)

    def _scan_triple_string(self, sl: int, sc: int) -> None:
        # Consume opening """
        self._advance()
        self._advance()
        self._advance()

        buf: list[str] = []
        has_interp = False

        while not self._at_end():
            if self._peek() == '"' and self._peek(1) == '"' and self._peek(2) == '"':
                break

            if self._peek() == "\\":
                buf.append(self._scan_escape())
            elif self._peek() == "{":
                has_interp = True
                self._emit(TokenType.STRING_PART, "".join(buf), sl, sc)
                buf = []
                interp_sl, interp_sc = self._line, self._col
                self._advance()
                self._emit(TokenType.INTERP_START, "{", interp_sl, interp_sc)
                self._scan_interpolation_body()
                if self._at_end() or self._peek() != "}":
                    raise self._error("unterminated string interpolation; expected '}'")
                close_sl, close_sc = self._line, self._col
                self._advance()
                self._emit(TokenType.INTERP_END, "}", close_sl, close_sc)
                sl, sc = self._line, self._col
            else:
                buf.append(self._advance())

        if self._at_end():
            raise self._error("unterminated triple-quoted string")

        # Consume closing """
        self._advance()
        self._advance()
        self._advance()

        if has_interp:
            if buf:
                self._emit(TokenType.STRING_PART, "".join(buf), sl, sc)
        else:
            self._emit(TokenType.STRING, "".join(buf), sl, sc)

    def _scan_interpolation_body(self) -> None:
        """Lex tokens inside a string interpolation until we see a matching '}'."""
        depth = 1
        while not self._at_end() and depth > 0:
            self._skip_whitespace_and_comments()
            if self._at_end():
                break
            ch = self._peek()
            if ch == "}":
                depth -= 1
                if depth == 0:
                    return  # caller will consume the }
                # Otherwise it belongs to a nested block
            elif ch == "{":
                depth += 1

            # Delegate to normal token scanning
            self._scan_token()

    # ------------------------------------------------------------------
    # Escape sequences
    # ------------------------------------------------------------------

    def _scan_escape(self) -> str:
        self._advance()  # backslash
        if self._at_end():
            raise self._error("unexpected end of file in escape sequence")
        ch = self._advance()
        simple = {
            "n": "\n", "r": "\r", "t": "\t", "\\": "\\",
            '"': '"', "'": "'", "0": "\0", "{": "{", "}": "}",
        }
        if ch in simple:
            return simple[ch]
        if ch == "u":
            return self._scan_unicode_escape()
        raise self._error(f"unknown escape sequence '\\{ch}'")

    def _scan_unicode_escape(self) -> str:
        if self._at_end() or self._peek() != "{":
            raise self._error("expected '{' after '\\u'")
        self._advance()  # {
        digits: list[str] = []
        while not self._at_end() and self._peek() != "}":
            digits.append(self._advance())
        if self._at_end():
            raise self._error("unterminated unicode escape")
        self._advance()  # }
        try:
            codepoint = int("".join(digits), 16)
            return chr(codepoint)
        except (ValueError, OverflowError):
            raise self._error(f"invalid unicode codepoint: {''.join(digits)}")

    # ------------------------------------------------------------------
    # Character literals
    # ------------------------------------------------------------------

    def _scan_char(self, sl: int, sc: int) -> None:
        self._advance()  # opening '
        if self._at_end():
            raise self._error("unterminated character literal")

        if self._peek() == "\\":
            value = self._scan_escape()
        elif self._peek() == "'":
            raise self._error("empty character literal")
        else:
            value = self._advance()

        if self._at_end() or self._peek() != "'":
            raise self._error("unterminated character literal; expected closing '")
        self._advance()  # closing '
        self._emit(TokenType.CHAR, value, sl, sc)

    # ------------------------------------------------------------------
    # Operators & punctuation
    # ------------------------------------------------------------------

    def _scan_operator(self, sl: int, sc: int) -> None:
        ch = self._advance()

        match ch:
            case "(":
                self._emit(TokenType.LPAREN, ch, sl, sc)
            case ")":
                self._emit(TokenType.RPAREN, ch, sl, sc)
            case "{":
                self._emit(TokenType.LBRACE, ch, sl, sc)
            case "}":
                self._emit(TokenType.RBRACE, ch, sl, sc)
            case "[":
                self._emit(TokenType.LBRACKET, ch, sl, sc)
            case "]":
                self._emit(TokenType.RBRACKET, ch, sl, sc)
            case ";":
                self._emit(TokenType.SEMICOLON, ch, sl, sc)
            case ",":
                self._emit(TokenType.COMMA, ch, sl, sc)
            case "~":
                self._emit(TokenType.TILDE, ch, sl, sc)

            case "+":
                if self._match("="):
                    self._emit(TokenType.PLUS_EQ, "+=", sl, sc)
                else:
                    self._emit(TokenType.PLUS, "+", sl, sc)

            case "-":
                if self._match(">"):
                    self._emit(TokenType.ARROW, "->", sl, sc)
                elif self._match("="):
                    self._emit(TokenType.MINUS_EQ, "-=", sl, sc)
                else:
                    self._emit(TokenType.MINUS, "-", sl, sc)

            case "*":
                if self._match("*"):
                    self._emit(TokenType.POWER, "**", sl, sc)
                elif self._match("="):
                    self._emit(TokenType.STAR_EQ, "*=", sl, sc)
                else:
                    self._emit(TokenType.STAR, "*", sl, sc)

            case "/":
                if self._match("="):
                    self._emit(TokenType.SLASH_EQ, "/=", sl, sc)
                else:
                    self._emit(TokenType.SLASH, "/", sl, sc)

            case "%":
                self._emit(TokenType.PERCENT, "%", sl, sc)

            case "=":
                if self._match("="):
                    self._emit(TokenType.EQ_EQ, "==", sl, sc)
                elif self._match(">"):
                    self._emit(TokenType.FAT_ARROW, "=>", sl, sc)
                else:
                    self._emit(TokenType.EQ, "=", sl, sc)

            case "!":
                if self._match("="):
                    self._emit(TokenType.BANG_EQ, "!=", sl, sc)
                else:
                    self._error_token(f"unexpected character '!'", sl, sc)

            case "<":
                if self._match("="):
                    self._emit(TokenType.LT_EQ, "<=", sl, sc)
                elif self._match("<"):
                    self._emit(TokenType.SHL, "<<", sl, sc)
                else:
                    self._emit(TokenType.LT, "<", sl, sc)

            case ">":
                if self._match("="):
                    self._emit(TokenType.GT_EQ, ">=", sl, sc)
                elif self._match(">"):
                    self._emit(TokenType.SHR, ">>", sl, sc)
                else:
                    self._emit(TokenType.GT, ">", sl, sc)

            case "&":
                self._emit(TokenType.AMP, "&", sl, sc)
            case "|":
                self._emit(TokenType.PIPE, "|", sl, sc)
            case "^":
                self._emit(TokenType.CARET, "^", sl, sc)

            case ".":
                if self._match("."):
                    if self._match("="):
                        self._emit(TokenType.DOT_DOT_EQ, "..=", sl, sc)
                    else:
                        self._emit(TokenType.DOT_DOT, "..", sl, sc)
                else:
                    self._emit(TokenType.DOT, ".", sl, sc)

            case ":":
                if self._match(":"):
                    self._emit(TokenType.COLON_COLON, "::", sl, sc)
                else:
                    self._emit(TokenType.COLON, ":", sl, sc)

            case "@":
                # Try to match an attribute keyword
                if self._peek().isalpha() or self._peek() == "_":
                    attr_start = self._pos
                    while not self._at_end() and (self._peek().isalnum() or self._peek() == "_"):
                        self._advance()
                    attr_name = self._src[attr_start:self._pos]
                    if attr_name in ATTRIBUTE_KEYWORDS:
                        self._emit(ATTRIBUTE_KEYWORDS[attr_name], f"@{attr_name}", sl, sc)
                    else:
                        # Not a recognized attribute -- emit @ as matrix-mul
                        # and rewind so the identifier is scanned normally
                        self._pos = attr_start
                        self._col = sc + 1
                        self._emit(TokenType.AT, "@", sl, sc)
                else:
                    self._emit(TokenType.AT, "@", sl, sc)

            case _:
                self._error_token(f"unexpected character {ch!r}", sl, sc)


# ---------------------------------------------------------------------------
# Convenience function
# ---------------------------------------------------------------------------

def tokenize(source: str, filename: str = "<stdin>") -> list[Token]:
    """Tokenize Kryos source code and return a list of tokens."""
    return Lexer(source, filename).tokenize()
