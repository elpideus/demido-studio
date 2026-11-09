"""Demido Studio's bridge to a MetaTrader 5 terminal: reads candles, never trades.

The `MetaTrader5` package exists for Windows only. On Windows this runs in a Python Demido keeps for
it; on Linux the terminal runs under Wine, and so does this script, in the terminal's own prefix (the
package talks to the terminal through that prefix's Windows). The market service starts it and
talks to it over stdin and stdout, one JSON object per line each way:

    {"id": 1, "method": "rates", "params": {...}}  ->  {"id": 1, "result": ...}
                                                    or {"id": 1, "error": {"code": "...", "message": "..."}}

One request at a time, in order. It ends when its stdin closes; before that it switches the terminal
back to the account it was on (when it switched it) and leaves the terminal running.

Methods:
    hello                         versions
    running {path}                {pid} of the terminal at that path, or {pid: null}
    launch {path, args}           starts it minimized without taking focus, outside Demido's job
                                  (so it outlives Demido); {pid, tied}
    connect {path, login, server, timeout, allowSwitch}
                                  attaches to the terminal and makes the account the active one,
                                  with the password the terminal saved; {account, terminal, switched}
    symbols                       [{name, path, description, base, profit, digits, visible}]
    rates {symbol, from, to, timeframe}
                                  M1 (or H1) bars with server times in [from, to) (server-clock seconds),
                                  asked once (once more after a pause when empty): {time, open, high,
                                  low, close, volume, last, connected, digits}. The terminal answers
                                  only within the newest "Max bars in chart" bars
    history {symbol, from, to}    M1 bars with server times in [from, to) read from the terminal's
                                  history files on disk, for what lies beyond Max bars; the terminal
                                  is asked nothing. Like rates, with last null, connected false and
                                  {files} read
    tick {symbol}                 {timeMsc, bid, ask} or null
    io {pid}                      {written} bytes the process wrote (Windows), or {written: null}
    restore                       back to the account the terminal was on before `connect`

Stdlib and MetaTrader5 only. `python -I bridge.py --site <dir>` puts the package's folder on the path.
"""

from __future__ import annotations

import json
import os
import struct
import sys
import time
from datetime import datetime, timezone

VERSION = 1
# The protocol owns stdout; anything the package prints goes to stderr.
OUT = sys.stdout
sys.stdout = sys.stderr

mt5 = None
state = {"previous": None, "selected": set(), "path": None}


class Failure(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


def package():
    global mt5
    if mt5 is None:
        try:
            import MetaTrader5 as module
        except ImportError as error:
            raise Failure("NO_PACKAGE", f"The MetaTrader5 package is not installed: {error}") from error
        mt5 = module
    return mt5


def last_error() -> str:
    try:
        code, message = package().last_error()
        return f"{message} ({code})"
    except Exception:  # noqa: BLE001 - only for a message
        return "unknown error"


# ---------------------------------------------------------------------------------------------
# Windows processes (ctypes; under Wine too)


def kernel32():
    if sys.platform != "win32":
        raise Failure("UNSUPPORTED", "Only the Windows side of the bridge manages the terminal's process")
    import ctypes
    from ctypes import wintypes

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = wintypes.HANDLE
    k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    k32.CloseHandle.argtypes = [wintypes.HANDLE]
    k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    k32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    k32.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, wintypes.PDWORD]
    k32.GetProcessIoCounters.argtypes = [wintypes.HANDLE, ctypes.c_void_p]
    return k32


def user32():
    import ctypes
    from ctypes import wintypes

    u32 = ctypes.WinDLL("user32", use_last_error=True)
    u32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, wintypes.LPDWORD]
    u32.IsWindowVisible.argtypes = [wintypes.HWND]
    u32.IsIconic.argtypes = [wintypes.HWND]
    u32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
    u32.GetWindow.restype = wintypes.HWND
    u32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    return u32


def image_path(k32, pid: int) -> str | None:
    import ctypes
    from ctypes import wintypes

    handle = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return None
    try:
        size = wintypes.DWORD(1024)
        buffer = ctypes.create_unicode_buffer(size.value)
        if not k32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            return None
        return buffer.value
    finally:
        k32.CloseHandle(handle)


def find_process(path: str) -> int | None:
    import ctypes
    from ctypes import wintypes

    class Entry(ctypes.Structure):
        _fields_ = [
            ("dwSize", wintypes.DWORD),
            ("cntUsage", wintypes.DWORD),
            ("th32ProcessID", wintypes.DWORD),
            ("th32DefaultHeapID", ctypes.c_size_t),
            ("th32ModuleID", wintypes.DWORD),
            ("cntThreads", wintypes.DWORD),
            ("th32ParentProcessID", wintypes.DWORD),
            ("pcPriClassBase", wintypes.LONG),
            ("dwFlags", wintypes.DWORD),
            ("szExeFile", wintypes.WCHAR * 260),
        ]

    k32 = kernel32()
    k32.Process32FirstW.argtypes = k32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(Entry)]
    snapshot = k32.CreateToolhelp32Snapshot(0x2, 0)  # TH32CS_SNAPPROCESS
    if not snapshot or snapshot == ctypes.c_void_p(-1).value:
        raise Failure("ERROR", f"Could not list processes (error {ctypes.get_last_error()})")
    want = os.path.normcase(os.path.normpath(path))
    name = os.path.basename(want)
    try:
        entry = Entry()
        entry.dwSize = ctypes.sizeof(Entry)
        more = k32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            if entry.szExeFile.lower() == name:
                image = image_path(k32, entry.th32ProcessID)
                if image and os.path.normcase(os.path.normpath(image)) == want:
                    return int(entry.th32ProcessID)
            more = k32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        k32.CloseHandle(snapshot)
    return None


def windows_of(pid: int) -> list[int]:
    import ctypes
    from ctypes import wintypes

    u32 = user32()
    found: list[int] = []
    proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def visit(hwnd, _lparam):
        owner = wintypes.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and u32.IsWindowVisible(hwnd) and not u32.GetWindow(hwnd, 4):  # GW_OWNER
            found.append(hwnd)
        return True

    u32.EnumWindows(proc(visit), 0)
    return found


def keep_minimized(pid: int, seconds: float) -> None:
    """The terminal restores the window placement it saved; put its main window back to minimized,
    without activating it, while it starts."""
    u32 = user32()
    deadline = time.monotonic() + seconds
    settled = 0
    while time.monotonic() < deadline:
        hwnds = windows_of(pid)
        for hwnd in hwnds:
            if not u32.IsIconic(hwnd):
                u32.ShowWindow(hwnd, 7)  # SW_SHOWMINNOACTIVE
        settled = settled + 1 if hwnds and all(u32.IsIconic(h) for h in hwnds) else 0
        if settled >= 8:
            return
        time.sleep(0.25)


def running(params: dict) -> dict:
    return {"pid": find_process(str(params["path"]))}


def launch(params: dict) -> dict:
    import subprocess

    path = str(params["path"])
    pid = find_process(path)
    if pid is not None:
        return {"pid": pid, "tied": False, "launched": False}
    info = subprocess.STARTUPINFO()
    info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    info.wShowWindow = 7  # SW_SHOWMINNOACTIVE
    base = subprocess.CREATE_NEW_PROCESS_GROUP | 0x00000008  # DETACHED_PROCESS
    args = [path, *[str(a) for a in params.get("args") or []]]
    common = {
        "startupinfo": info,
        "cwd": os.path.dirname(path),
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "close_fds": True,
    }
    tied = False
    try:
        # Out of Demido's job object, so closing Demido does not close the terminal.
        child = subprocess.Popen(args, creationflags=base | 0x01000000, **common)  # CREATE_BREAKAWAY_FROM_JOB
    except OSError:
        child = subprocess.Popen(args, creationflags=base, **common)
        tied = True
    keep_minimized(child.pid, float(params.get("settle", 20)))
    return {"pid": child.pid, "tied": tied, "launched": True}


def io(params: dict) -> dict:
    import ctypes

    if sys.platform != "win32":
        return {"written": None}

    class Counters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in ("ro", "wo", "oo", "rb", "wb", "ob")]

    k32 = kernel32()
    handle = k32.OpenProcess(0x1000, False, int(params["pid"]))
    if not handle:
        return {"written": None}
    try:
        counters = Counters()
        if not k32.GetProcessIoCounters(handle, ctypes.byref(counters)):
            return {"written": None}
        # Wine answers zeros: no measurement there.
        return {"written": int(counters.wb) or None}
    finally:
        k32.CloseHandle(handle)


# ---------------------------------------------------------------------------------------------
# The terminal


def fields(value, names: tuple[str, ...]) -> dict:
    return {name: getattr(value, name, None) for name in names}


ACCOUNT = ("login", "server", "name", "company", "currency", "trade_mode")
TERMINAL = ("connected", "build", "maxbars", "path", "data_path", "company", "name", "trade_allowed")


def attach(path: str, timeout: int) -> None:
    m = package()
    if m.terminal_info() is not None and state["path"] == path:
        return
    if not m.initialize(path=path, timeout=timeout):
        raise Failure("INIT_FAILED", f"MetaTrader did not answer: {last_error()}")
    state["path"] = path


def wait_account(seconds: float):
    """The account the terminal is logged into; a terminal that just started logs in on its own."""
    m = package()
    deadline = time.monotonic() + seconds
    while True:
        account = m.account_info()
        if account is not None and account.login:
            return account
        if time.monotonic() >= deadline:
            return None
        time.sleep(0.5)


def connect(params: dict) -> dict:
    m = package()
    path = str(params["path"])
    login = int(params["login"])
    server = str(params["server"])
    timeout = int(params.get("timeout", 60_000))
    attach(path, timeout)
    current = wait_account(float(params.get("settle", 15)))
    switched = False
    if current is None or current.login != login or str(current.server) != server:
        if not params.get("allowSwitch", True):
            raise Failure("OTHER_ACCOUNT", "MetaTrader is logged into another account")
        if current is not None and state["previous"] is None:
            state["previous"] = {"login": int(current.login), "server": str(current.server)}
        # No password: the terminal uses the one it saved for this account.
        if not m.login(login, server=server, timeout=timeout):
            raise Failure(
                "AUTH_FAILED",
                f"MetaTrader could not log into {login} on {server}: {last_error()}. Log into it once in "
                "MetaTrader with \"Save password\" ticked.",
            )
        switched = True
    account = m.account_info()
    terminal = m.terminal_info()
    return {
        "account": fields(account, ACCOUNT) if account else None,
        "terminal": fields(terminal, TERMINAL) if terminal else None,
        "switched": switched,
    }


def restore(_params: dict | None = None) -> dict:
    previous = state["previous"]
    restored = False
    if mt5 is not None:
        for name in sorted(state["selected"]):
            try:
                mt5.symbol_select(name, False)
            except Exception:  # noqa: BLE001 - courtesy only
                pass
        state["selected"].clear()
        if previous is not None:
            account = mt5.account_info()
            if account is None or account.login != previous["login"]:
                restored = bool(mt5.login(previous["login"], server=previous["server"]))
            state["previous"] = None
    return {"restored": restored, "previous": previous}


def symbols(_params: dict) -> list[dict]:
    got = package().symbols_get()
    if got is None:
        raise Failure("ERROR", f"MetaTrader listed no symbols: {last_error()}")
    return [
        {
            "name": s.name,
            "path": getattr(s, "path", ""),
            "description": getattr(s, "description", ""),
            "base": getattr(s, "currency_base", ""),
            "profit": getattr(s, "currency_profit", ""),
            "digits": int(getattr(s, "digits", 0)),
            "visible": bool(getattr(s, "visible", False)),
        }
        for s in got
    ]


def column(got, name: str) -> list:
    if hasattr(got, "dtype"):
        return got[name].tolist()
    return [row[name] for row in got]


def symbol_of(name: str):
    info = package().symbol_info(name)
    if info is None:
        raise Failure("NO_SYMBOL", f"MetaTrader has no symbol {name}")
    return info


def rates(params: dict) -> dict:
    m = package()
    name = str(params["symbol"])
    start, end = int(params["from"]), int(params["to"])
    frame = {"M1": m.TIMEFRAME_M1, "H1": m.TIMEFRAME_H1}.get(str(params.get("timeframe", "M1")))
    if frame is None:
        raise Failure("ERROR", f"Unknown timeframe {params.get('timeframe')}")
    info = symbol_of(name)
    if not info.visible:
        if not m.symbol_select(name, True):
            raise Failure("NO_SYMBOL", f"MetaTrader cannot select {name}: {last_error()}")
        state["selected"].add(name)
    # The package sends a datetime as epoch seconds, which the terminal reads on its own clock.
    lo = datetime.fromtimestamp(start, timezone.utc)
    hi = datetime.fromtimestamp(end, timezone.utc)
    got = m.copy_rates_range(name, frame, lo, hi)
    if got is None or len(got) == 0:
        # The terminal may have had to download it first: once more, never in a loop.
        time.sleep(float(params.get("pause", 5)))
        got = m.copy_rates_range(name, frame, lo, hi)
    digits = int(info.digits)
    out = {key: [] for key in ("time", "open", "high", "low", "close", "volume")}
    if got is not None and len(got) > 0:
        times = [int(t) for t in column(got, "time")]
        # Before a symbol's history starts the terminal answers with its first bar: keep the range.
        keep = [i for i, t in enumerate(times) if start <= t < end]
        out["time"] = [times[i] for i in keep]
        for field in ("open", "high", "low", "close"):
            values = column(got, field)
            out[field] = [round(float(values[i]), digits) for i in keep]
        volumes = column(got, "tick_volume")
        out["volume"] = [int(volumes[i]) for i in keep]
    fresh = m.symbol_info(name) or info
    terminal = m.terminal_info()
    out["last"] = int(getattr(fresh, "time", 0) or 0) or None
    out["connected"] = bool(terminal.connected) if terminal is not None else False
    out["digits"] = digits
    return out


# ---------------------------------------------------------------------------------------------
# The terminal's history files: what lies beyond "Max bars in chart"

# Layout 502 of <data path>/bases/<server>/history/<symbol>/<year>.hcc, read from MetaQuotes-Demo's
# files (2026-10): a 228-byte header (the version, a copyright, "History", the symbol); a table of
# 18-byte entries, one a server day with bars (its number, its bar count, three words, its block's
# offset), in the order the days were stored (newest first for history fetched backwards); each
# block is a 189-byte header naming the symbol and that many 60-byte bars (MqlRates, packed). The
# first entry that does not hold together ends the table. Older history is kept a day a bar: one bar
# at midnight.
HCC_VERSION = 502
HCC_TABLE = 228
HCC_ENTRY = struct.Struct("<HHIHHHI")
HCC_BLOCK = 189
# time, open, high, low, close, tick volume, spread, real volume
HCC_BAR = struct.Struct("<qddddqiq")
DAY = 86400


def read_hcc(path: str, symbol: str, start: int, end: int) -> tuple[list[tuple], int | None]:
    """One file's bars with server times in [start, end), and its last day kept as one bar (None
    when it has none). A file that cannot be read has nothing."""
    try:
        with open(path, "rb") as f:
            raw = f.read()
    except OSError:
        return [], None
    if len(raw) < HCC_TABLE:
        return [], None
    version = struct.unpack_from("<I", raw, 0)[0]
    if version != HCC_VERSION:
        raise Failure("HISTORY_LAYOUT", f"MetaTrader's history files have a layout Demido does not read ({version})")
    tag = symbol.encode("utf-16-le") + b"\0\0"
    view = memoryview(raw)
    bars: list[tuple] = []
    daily = None
    at = HCC_TABLE
    while at + HCC_ENTRY.size <= len(raw):
        _day, n, _stored, _a, _b, _c, pos = HCC_ENTRY.unpack_from(raw, at)
        stop = pos + HCC_BLOCK + n * HCC_BAR.size
        if not 0 < n <= 1440 or stop > len(raw) or raw[pos + 2 : pos + 2 + len(tag)] != tag:
            break
        at += HCC_ENTRY.size
        # A block is one server day: only the days of the range are unpacked.
        first = struct.unpack_from("<q", raw, pos + HCC_BLOCK)[0]
        if n == 1 and first % DAY == 0:
            daily = first if daily is None else max(daily, first)
            continue
        if first >= end or first - first % DAY + DAY <= start:
            continue
        bars.extend(b for b in HCC_BAR.iter_unpack(view[pos + HCC_BLOCK : stop]) if start <= b[0] < end)
    return bars, daily


def history(params: dict) -> dict:
    m = package()
    name = str(params["symbol"])
    start, end = int(params["from"]), int(params["to"])
    info = symbol_of(name)
    terminal, account = m.terminal_info(), m.account_info()
    data = getattr(terminal, "data_path", None) if terminal is not None else None
    if not data or account is None:
        raise Failure("NO_HISTORY", "MetaTrader did not say where it keeps its history")
    folder = os.path.join(str(data), "bases", str(account.server), "history", name)
    first = datetime.fromtimestamp(start, timezone.utc).year
    last = datetime.fromtimestamp(max(start, end - 1), timezone.utc).year
    bars: list[tuple] = []
    daily = None
    files = 0
    for year in range(first, last + 1):
        path = os.path.join(folder, f"{year}.hcc")
        if not os.path.isfile(path):
            continue
        files += 1
        got, d = read_hcc(path, name, start, end)
        bars.extend(got)
        if d is not None:
            daily = d if daily is None else max(daily, d)
    if daily is not None:
        # Minutes start after the last day kept as one bar.
        bars = [b for b in bars if b[0] >= daily + DAY]
    bars.sort(key=lambda b: b[0])
    digits = int(info.digits)
    out = {key: [] for key in ("time", "open", "high", "low", "close", "volume")}
    previous = None
    for t, o, h, lo, c, v, _spread, _real in bars:
        if t == previous or t % 60 or not (0 < lo <= min(o, c) and max(o, c) <= h):
            continue
        previous = t
        out["time"].append(int(t))
        out["open"].append(round(o, digits))
        out["high"].append(round(h, digits))
        out["low"].append(round(lo, digits))
        out["close"].append(round(c, digits))
        out["volume"].append(int(v))
    out["last"] = None
    out["connected"] = False
    out["digits"] = digits
    out["files"] = files
    return out


def tick(params: dict) -> dict | None:
    got = package().symbol_info_tick(str(params["symbol"]))
    if got is None:
        return None
    return {"timeMsc": int(got.time_msc), "bid": float(got.bid), "ask": float(got.ask)}


def hello(_params: dict) -> dict:
    version = None
    try:
        version = getattr(package(), "__version__", None)
    except Failure:
        pass
    return {"bridge": VERSION, "python": sys.version.split()[0], "platform": sys.platform, "package": version}


METHODS = {
    "hello": hello,
    "running": running,
    "launch": launch,
    "connect": connect,
    "symbols": symbols,
    "rates": rates,
    "history": history,
    "tick": tick,
    "io": io,
    "restore": restore,
}


def send(message: dict) -> None:
    OUT.write(json.dumps(message, separators=(",", ":")) + "\n")
    OUT.flush()


def serve() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
            ident = request["id"]
        except (ValueError, KeyError, TypeError):
            continue
        method = METHODS.get(request.get("method"))
        try:
            if method is None:
                raise Failure("UNKNOWN_METHOD", f"Unknown method {request.get('method')}")
            send({"id": ident, "result": method(request.get("params") or {})})
        except Failure as error:
            send({"id": ident, "error": {"code": error.code, "message": str(error)}})
        except Exception as error:  # noqa: BLE001 - every failure goes back to the caller
            send({"id": ident, "error": {"code": "ERROR", "message": f"{type(error).__name__}: {error}"}})


def main() -> None:
    args = sys.argv[1:]
    if "--site" in args:
        sys.path.insert(0, args[args.index("--site") + 1])
    try:
        serve()
    finally:
        try:
            restore()
            if mt5 is not None:
                mt5.shutdown()
        except Exception:  # noqa: BLE001 - leaving anyway
            pass


if __name__ == "__main__":
    main()
