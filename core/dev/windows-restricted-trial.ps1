# The second half of the Windows sandbox trial (research note 29): can a program running as the sandbox user
# start a command whose writes are kept to the session's folder? It makes a *write-restricted* token from its
# own, carrying a SID made up for the session, and starts the command with it (CreateProcessAsUserW needs no
# special right for a restricted copy of the caller's own token). Windows then asks for that SID too whenever
# something is to be changed, and only the session's folder grants it. Run after windows-sandbox-trial.ps1,
# on the checks' machine, as an administrator.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = 'C:\demido-trial'
$work = Join-Path $root 'out'

function Answer($question, $yes, $detail) {
    $mark = if ($yes) { 'yes' } else { 'no ' }
    Write-Host ("{0,-78} {1}  {2}" -f $question, $mark, $detail)
}

# A user to be the sandbox, as in the first half.
$bytes = New-Object byte[] 24
(New-Object Security.Cryptography.RNGCryptoServiceProvider).GetBytes($bytes)
$plain = [Convert]::ToBase64String($bytes) + 'aZ7!'
$secure = ConvertTo-SecureString $plain -AsPlainText -Force
$name = 'DemidoTrial2'
New-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword | Out-Null
$cred = New-Object Management.Automation.PSCredential($name, $secure)

# The SID made up for the session: never a real account's.
$r = New-Object Random
$cap = "S-1-5-21-$($r.Next(1, 2147483647))-$($r.Next(1, 2147483647))-$($r.Next(1, 2147483647))-$($r.Next(1000, 2147483647))"
$session = Join-Path $root 'session2'
$other = Join-Path $root 'other2'   # shared with the user, but not with the session's SID: as another session's folder
$public = 'C:\demido-trial-public'  # made at C:\ with Windows's default permissions
foreach ($d in @($session, $other, $public)) { New-Item -ItemType Directory -Force $d | Out-Null }
icacls $session /grant "$($name):(OI)(CI)M" /grant "*$($cap):(OI)(CI)M" | Out-Null
icacls $other /grant "$($name):(OI)(CI)M" | Out-Null
$scratch = Join-Path $root 'scratch2'      # the command's own temporary folder
New-Item -ItemType Directory -Force $scratch | Out-Null
icacls $scratch /grant "$($name):(OI)(CI)M" /grant "*$($cap):(OI)(CI)M" | Out-Null
icacls $work /grant "*$($cap):(OI)(CI)M" | Out-Null

# The runner: C#, compiled by PowerShell, run as the sandbox user.
$runner = Join-Path $root 'runner.ps1'
@'
param([string]$cap, [string]$lineFile, [string]$dir, [string]$out)
$line = (Get-Content $lineFile -Raw).Trim()
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class R {
  [StructLayout(LayoutKind.Sequential)] public struct SID_AND_ATTRIBUTES { public IntPtr Sid; public uint Attributes; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct STARTUPINFO { public int cb; public string r; public string d; public string t; public int x, y, w, h, cx, cy, fill, flags; public short show, r2; public IntPtr r3, i, o, e; }
  [StructLayout(LayoutKind.Sequential)] public struct PI { public IntPtr p, t; public int pid, tid; }
  [DllImport("advapi32", SetLastError = true)] public static extern bool OpenProcessToken(IntPtr p, uint a, out IntPtr t);
  [DllImport("advapi32", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool ConvertStringSidToSid(string s, out IntPtr sid);
  [DllImport("advapi32", SetLastError = true)] public static extern bool CreateRestrictedToken(IntPtr t, uint flags, uint nd, IntPtr d, uint np, IntPtr p, uint nr, SID_AND_ATTRIBUTES[] r, out IntPtr nt);
  [DllImport("advapi32", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool CreateProcessAsUserW(IntPtr t, string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PI pi);
  [DllImport("kernel32")] public static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32")] public static extern bool GetExitCodeProcess(IntPtr h, out uint code);
  public static string Run(string cap, string cmd, string dir) {
    IntPtr own, sid, restricted;
    if (!OpenProcessToken(GetCurrentProcess(), 0x0002 | 0x0001 | 0x0008 | 0x0080, out own)) return "OpenProcessToken " + Marshal.GetLastWin32Error();
    if (!ConvertStringSidToSid(cap, out sid)) return "ConvertStringSidToSid " + Marshal.GetLastWin32Error();
    var list = new SID_AND_ATTRIBUTES[] { new SID_AND_ATTRIBUTES { Sid = sid, Attributes = 0 } };
    // WRITE_RESTRICTED (8): the restricting SIDs are asked for only when something is to be changed.
    if (!CreateRestrictedToken(own, 8, 0, IntPtr.Zero, 0, IntPtr.Zero, 1, list, out restricted)) return "CreateRestrictedToken " + Marshal.GetLastWin32Error();
    var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si);
    PI pi;
    if (!CreateProcessAsUserW(restricted, null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0x08000000, IntPtr.Zero, dir, ref si, out pi)) return "CreateProcessAsUserW " + Marshal.GetLastWin32Error();
    WaitForSingleObject(pi.p, 60000);
    uint code; GetExitCodeProcess(pi.p, out code);
    return "exit " + code;
  }
}
"@
$said = [R]::Run($cap, "cmd.exe /D /S /C `"$line > `"$out`" 2>&1`"", $dir)
Write-Output $said
'@ | Set-Content $runner
icacls $root /grant "$($name):(RX)" | Out-Null
icacls $runner /grant "$($name):(RX)" | Out-Null

$script:n = 0
function Restricted($line, $dir = $session) {
    $script:n++
    $out = Join-Path $work "r-$($script:n).txt"
    $log = Join-Path $work "rl-$($script:n).txt"
    # The command goes in a file: quotes inside it would not survive being passed on as an argument.
    $lineFile = Join-Path $work "line-$($script:n).txt"
    Set-Content $lineFile $line
    $p = Start-Process powershell.exe -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $runner, $cap, $lineFile, $dir, $out `
        -Credential $cred -WorkingDirectory $dir -RedirectStandardOutput $log -NoNewWindow -Wait -PassThru -LoadUserProfile
    $said = (Get-Content $log -Raw -ErrorAction SilentlyContinue) + ' ' + (Get-Content $out -Raw -ErrorAction SilentlyContinue)
    ($said -replace '\s+', ' ').Trim()
}

$t = Restricted 'whoami'
Answer 'The runner, as the sandbox user, starts a command with a write-restricted token' ($t -match 'demidotrial2') $t
$t = Restricted "echo in> `"$session\in.txt`" && type `"$session\in.txt`""
Answer 'The command can write in the session folder (it grants the session SID)' ($t -match 'in') $t
$t = Restricted "echo out> `"$public\out.txt`""
Answer "It cannot write in a folder at C:\ that every user may change" (-not (Test-Path "$public\out.txt")) $t
$t = Restricted "echo out> `"$other\out.txt`""
Answer 'Nor in another folder shared with the same user (another session)' (-not (Test-Path "$other\out.txt")) $t
Set-Content "$other\theirs.txt" 'another session''s'
$t = Restricted "type `"$other\theirs.txt`""
Answer 'But it can read that folder (reads are not restricted)' ($t -match 'another') $t
$t = Restricted "set TEMP=$scratch&& set TMP=$scratch&& echo t> `"$scratch\t.txt`" && type `"$scratch\t.txt`""
Answer 'Its own temporary folder works when it grants the session SID' ($t -match '\bt\b') $t
$t = Restricted "echo p> `"%USERPROFILE%\p.txt`" && type `"%USERPROFILE%\p.txt`""
Answer "It cannot write in its user's own profile (not granted the session SID)" ($t -notmatch '\bp\b') $t
$t = Restricted 'reg add HKCU\Software\DemidoTrial /v x /d y /f'
Answer "It cannot change its user's own registry either" ($t -notmatch 'success') $t

# ---------- taking the entry off again, on a folder only the person may use ----------
# The first half's session folder sat at C:\, where every user may change things, so taking the entry off
# proved nothing there. Here the folder keeps no inherited entries: the person, SYSTEM and the administrators.
function As($line, $dir = $work) {
    $script:n++
    $out = Join-Path $work "a-$($script:n).txt"
    $err = Join-Path $work "ae-$($script:n).txt"
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/D', '/S', '/C', "`"$line`"" -Credential $cred -WorkingDirectory $dir `
        -RedirectStandardOutput $out -RedirectStandardError $err -NoNewWindow -Wait -PassThru -LoadUserProfile
    (((Get-Content $out -Raw -ErrorAction SilentlyContinue) + (Get-Content $err -Raw -ErrorAction SilentlyContinue)) -replace '\s+', ' ').Trim()
}
$locked = Join-Path $root 'locked'
New-Item -ItemType Directory -Force $locked | Out-Null
icacls $locked /inheritance:r /grant "$($env:USERNAME):(OI)(CI)F" /grant 'SYSTEM:(OI)(CI)F' /grant 'Administrators:(OI)(CI)F' | Out-Null
$t = As "echo x> `"$locked\early.txt`""
Answer 'It cannot write in a folder only the person may use' (-not (Test-Path "$locked\early.txt")) $t
icacls $locked /grant "$($name):(OI)(CI)M" | Out-Null
$t = As "echo made> `"$locked\made.txt`""
Answer 'Shared with it, it can' ((Get-Content "$locked\made.txt" -Raw -ErrorAction SilentlyContinue) -match 'made') $t
icacls $locked /remove:g $name /T | Out-Null
$t = As "echo again> `"$locked\made.txt`""
Answer 'With the entry taken off, it cannot write to the file it made' ((Get-Content "$locked\made.txt" -Raw) -notmatch 'again') $t
$t = As "type `"$locked\made.txt`""
Answer 'Nor read it' ($t -notmatch 'made') $t
$t = As "icacls `"$locked\made.txt`" /grant $($name):F"
$t2 = As "echo back> `"$locked\made.txt`""
Answer 'But as its owner it can give itself access back, and write' ((Get-Content "$locked\made.txt" -Raw) -match 'back') "$t | $t2"
icacls "$locked\made.txt" /reset | Out-Null
icacls $locked /setowner $env:USERNAME /T | Out-Null
$t = As "icacls `"$locked\made.txt`" /grant $($name):F"
$t2 = As "echo third> `"$locked\made.txt`""
Answer 'Once the person owns everything there again, it cannot' ((Get-Content "$locked\made.txt" -Raw) -notmatch 'third') "$t | $t2"

# ---------- loopback ----------
# A block rule for any address did not stop loopback in the first half. Codex names loopback outright, by protocol.
$sid = (Get-LocalUser $name).SID.Value
$sddl = "O:LSD:(A;;CC;;;$sid)"
New-NetFirewallRule -Name 'DemidoTrial2Out' -DisplayName 'Demido trial 2: no network out' -Direction Outbound -Action Block -LocalUser $sddl -Profile Any | Out-Null
$listener = Start-Job { $l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 47322); $l.Start(); while ($true) { $c = $l.AcceptTcpClient(); $s = $c.GetStream(); $b = [Text.Encoding]::ASCII.GetBytes("HTTP/1.0 200 OK`r`nContent-Length: 2`r`n`r`nok"); $s.Write($b, 0, $b.Length); $c.Close() } }
Start-Sleep 2
$t = As 'curl.exe -s -m 5 http://127.0.0.1:47322/'
Answer 'With only the any-address rule, it reaches loopback' ($t -match 'ok') $t
New-NetFirewallRule -Name 'DemidoTrial2LoopTcp' -DisplayName 'Demido trial 2: no loopback TCP' -Direction Outbound -Action Block -Protocol TCP -RemoteAddress '127.0.0.0/8', '::1' -LocalUser $sddl -Profile Any | Out-Null
New-NetFirewallRule -Name 'DemidoTrial2LoopUdp' -DisplayName 'Demido trial 2: no loopback UDP' -Direction Outbound -Action Block -Protocol UDP -RemoteAddress '127.0.0.0/8', '::1' -LocalUser $sddl -Profile Any | Out-Null
Start-Sleep 2
$t = As 'curl.exe -s -m 5 http://127.0.0.1:47322/'
Answer 'With loopback named, by protocol, it cannot reach a server on this computer' ($t -notmatch 'ok') $t
$mine = try { (Invoke-WebRequest -UseBasicParsing -TimeoutSec 5 http://127.0.0.1:47322/).Content } catch { $_.Exception.Message }
Answer 'The person still can' ($mine -match 'ok') $mine
Stop-Job $listener; Remove-Job $listener -Force
