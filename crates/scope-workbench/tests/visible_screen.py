"""Bounded VT cell reconstruction for Scope PTYs, not a general emulator.

Supports Scope's cursor, erase, SGR, mode and charset output plus literal HT,
wide BMP characters and combining accents. Rejects unmodelled controls rather
than silently manufacturing visual evidence. No emoji/ZWJ width claims.
"""
import codecs
import re
import unicodedata


class VisibleScreen:
    def __init__(self, rows=44, cols=180):
        self.pending = ''
        self.decoder = codecs.getincrementaldecoder('utf-8')('strict')
        self.resize(rows, cols)

    def resize(self, rows, cols):
        assert 1 <= rows <= 200 and 1 <= cols <= 500
        self.rows, self.cols = rows, cols
        self.cells = [[' '] * cols for _ in range(rows)]
        self.row = self.col = 0
        self.wrap = False

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        assert len(self.pending) <= 1_000_000, 'unbounded terminal sequence'
        while self.pending:
            if self.pending.startswith('\x1b['):
                match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', self.pending)
                if not match:
                    assert len(self.pending) < 128, repr(self.pending[:128])
                    return
                params, intermediate, action = match.groups()
                self.pending = self.pending[match.end():]
                if params.startswith('?') and action in 'hl':
                    # Alternate screen entry starts a fresh visible buffer.
                    if params == '?1049' and action == 'h':
                        self.resize(self.rows, self.cols)
                    continue
                if action in 'mnq' or (action in 'hl' and not params.startswith('?')):
                    continue  # styles, queries, cursor style, terminal modes
                assert not intermediate and re.fullmatch(r'[0-9;]*', params), (params, action)
                nums = [int(n or '0') for n in params.split(';')]
                n = nums[0] or 1
                if action in 'Hf':
                    self.row = min(self.rows - 1, n - 1)
                    self.col = min(self.cols - 1, (nums[1] or 1) - 1) if len(nums) > 1 else 0
                elif action == 'A': self.row = max(0, self.row - n)
                elif action == 'B': self.row = min(self.rows - 1, self.row + n)
                elif action == 'C': self.col = min(self.cols - 1, self.col + n)
                elif action == 'D': self.col = max(0, self.col - n)
                elif action == 'G': self.col = min(self.cols - 1, n - 1)
                elif action == 'd': self.row = min(self.rows - 1, n - 1)
                elif action == 'J':
                    mode = nums[0]
                    assert mode in (0, 1, 2, 3)
                    for r in range(self.rows):
                        for c in range(self.cols):
                            if mode in (2, 3) or (mode == 0 and (r, c) >= (self.row, self.col)) or (mode == 1 and (r, c) <= (self.row, self.col)):
                                self.cells[r][c] = ' '
                elif action == 'K':
                    assert nums[0] in (0, 1, 2)
                    start = 0 if nums[0] in (1, 2) else self.col
                    end = self.cols if nums[0] in (0, 2) else self.col + 1
                    self.cells[self.row][start:end] = [' '] * (end - start)
                else:
                    raise AssertionError(f'unmodelled CSI: {params!r} {action!r}')
                self.wrap = False
                continue
            if self.pending.startswith('\x1b'):
                if len(self.pending) < 2:
                    return
                assert self.pending[1] in '()', repr(self.pending[:30])
                if len(self.pending) < 3:
                    return
                self.pending = self.pending[3:]
                continue
            ch, self.pending = self.pending[0], self.pending[1:]
            if ch == '\r': self.col, self.wrap = 0, False
            elif ch == '\n': self._newline()
            elif ch == '\b': self.col, self.wrap = max(0, self.col - 1), False
            elif ch == '\t': self.col, self.wrap = min(self.cols - 1, (self.col // 8 + 1) * 8), False
            elif ch == '\x07': pass
            elif unicodedata.combining(ch):
                c = self.col if self.wrap else self.col - 1
                while c > 0 and self.cells[self.row][c] == '':
                    c -= 1
                if c >= 0:
                    self.cells[self.row][c] += ch
            else:
                assert ch >= ' ' and ch != '\x7f', repr(ch)
                width = 2 if unicodedata.east_asian_width(ch) in 'WF' else 1
                if self.wrap or self.col + width > self.cols:
                    self.col = 0
                    self._newline()
                self.cells[self.row][self.col] = ch
                if width == 2:
                    self.cells[self.row][self.col + 1] = ''
                self.col += width
                self.wrap = self.col == self.cols
                self.col = min(self.col, self.cols - 1)

    def _newline(self):
        self.row += 1
        self.wrap = False
        if self.row == self.rows:
            self.cells.pop(0)
            self.cells.append([' '] * self.cols)
            self.row -= 1

    def text(self):
        return '\n'.join(''.join(row) for row in self.cells)
