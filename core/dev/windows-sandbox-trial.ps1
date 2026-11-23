# What a user of Demido's own keeps a program from, on Windows (DECISIONS T26, research note 29).
#
# Run by the checks on their Windows machine, by hand (`gh workflow run Checks --ref <branch>`), as an
# administrator, which is what Demido's setup would be once. It makes two users, runs commands as one of them
# the way Demido would start a program there (Start-Process -Credential is CreateProcessWithLogonW, the same
# call), and says for each question what Windows did. Nothing here is meant for a person's own computer: it
# makes users and firewall rules and does not take them away.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File core/dev/windows-sandbox-trial.ps1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = 'C:\demido-trial'
$work = Join-Path $root 'out'
New-Item -ItemType Directory -Force $work | Out-Null
icacls $work /grant '*S-1-1-0:(OI)(CI)M' | Out-Null   # where the trial's own notes go: everyone may write here
$script:n = 0

function Answer($question, $yes, $detail) {
    $mark = if ($yes) { 'yes' } else { 'no ' }
    Write-Host ("{0,-78} {1}  {2}" -f $question, $mark, $detail)
}

function New-Password {
    $bytes = New-Object byte[] 24
    (New-Object Security.Cryptography.RNGCryptoServiceProvider).GetBytes($bytes)
    # Long and random; the letters, digit and sign at the end meet any password rule Windows may have.
    [Convert]::ToBase64String($bytes) + 'aZ7!'
}

function New-TrialUser($name) {
    $plain = New-Password
    $secure = ConvertTo-SecureString $plain -AsPlainText -Force
    New-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword -Description 'Demido sandbox trial' | Out-Null
    try { Add-LocalGroupMember -Group 'Users' -Member $name } catch { }
    # Not on the sign-in screen.
    $list = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList'
    New-Item $list -Force | Out-Null
    New-ItemProperty $list -Name $name -Value 0 -PropertyType DWord -Force | Out-Null
    [pscustomobject]@{ Name = $name; Plain = $plain; Credential = New-Object Management.Automation.PSCredential($name, $secure); Sid = (Get-LocalUser $name).SID.Value }
}

# Runs a cmd line as the user, in `dir`, and gives back what it printed and its exit code.
function As($user, $line, $dir = $work) {
    $script:n++
    $out = Join-Path $work "out-$($script:n).txt"
    $err = Join-Path $work "err-$($script:n).txt"
    try {
        $p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/D', '/S', '/C', "`"$line`"" -Credential $user.Credential -WorkingDirectory $dir `
            -RedirectStandardOutput $out -RedirectStandardError $err -NoNewWindow -Wait -PassThru -LoadUserProfile
        $text = ((Get-Content $out -Raw -ErrorAction SilentlyContinue) + (Get-Content $err -Raw -ErrorAction SilentlyContinue))
        [pscustomobject]@{ Code = $p.ExitCode; Text = ($text -replace '\s+', ' ').Trim() }
    } catch {
        [pscustomobject]@{ Code = -1; Text = "could not start: $($_.Exception.Message)" }
    }
}

Write-Host "Windows: $((Get-CimInstance Win32_OperatingSystem).Caption) $((Get-CimInstance Win32_OperatingSystem).Version); running as $env:USERNAME"
Write-Host "Firewall profiles: $((Get-NetFirewallProfile | ForEach-Object { "$($_.Name)=$($_.Enabled)" }) -join ', ')"
Write-Host ''

# ---------- the users ----------
$box = New-TrialUser 'DemidoTrial'
Answer 'A user of its own can be made, with a password nobody sees' $true "SID $($box.Sid)"

# The password, kept for this user only (DPAPI, the person's own scope), and given back.
Add-Type -AssemblyName System.Security
$blob = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($box.Plain), $null, 'CurrentUser')
$back = [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect($blob, $null, 'CurrentUser'))
Answer 'DPAPI keeps the password for the person only, and gives it back' ($back -eq $box.Plain) "$($blob.Length) bytes kept"

$who = As $box 'whoami'
Answer 'A program can be started as that user (CreateProcessWithLogonW)' ($who.Text -match 'demidotrial') $who.Text

# ---------- files ----------
$secret = Join-Path $env:USERPROFILE 'demido-trial-secret.txt'
Set-Content $secret 'the person''s own'
$r = As $box "type `"$secret`""
Answer "It cannot read a file in the person's profile ($env:USERPROFILE)" ($r.Text -notmatch 'own') $r.Text

$session = Join-Path $root 'session'
New-Item -ItemType Directory -Force $session | Out-Null
$r = As $box "echo made> `"$session\before.txt`""
Answer 'It cannot write in the session folder before the folder is shared' (-not (Test-Path "$session\before.txt")) $r.Text
icacls $session /grant "$($box.Name):(OI)(CI)M" | Out-Null
$r = As $box "echo made> `"$session\made.txt`" && type `"$session\made.txt`"" $session
Answer 'Once shared (one entry for the user), it can write there' ($r.Text -match 'made') $r.Text

# Outside the profiles, Windows's own defaults decide.
foreach ($drive in @('C:', 'D:')) {
    if (-not (Test-Path "$drive\")) { continue }
    $other = "$drive\demido-trial-other"
    New-Item -ItemType Directory -Force $other | Out-Null
    Set-Content "$other\theirs.txt" 'a project of the person''s'
    $r = As $box "type `"$other\theirs.txt`""
    Answer "It can read a folder the person made at $drive\ (default permissions)" ($r.Text -match 'project') $r.Text
    $r = As $box "echo changed> `"$other\theirs.txt`" && type `"$other\theirs.txt`""
    Answer "It can change a file there" ($r.Text -match 'changed') $r.Text
    Write-Host "    the folder's permissions: $((icacls $other | Select-Object -Skip 1 | Select-Object -First 6 | ForEach-Object { $_.Trim() }) -join ' | ')"
}

# Taking the entry off again.
icacls $session /remove:g $box.Name /T | Out-Null
$r = As $box "echo again> `"$session\made.txt`""
$after = (Get-Content "$session\made.txt" -Raw).Trim()
Answer 'With the entry taken off, it cannot write to the file it made' ($after -eq 'made') "file says: $after; $($r.Text)"
$r = As $box "icacls `"$session\made.txt`" /grant $($box.Name):F && echo back> `"$session\made.txt`""
$after = (Get-Content "$session\made.txt" -Raw).Trim()
Answer 'But it owns that file, and can give itself access back' ($after -eq 'back') "file says: $after"
Write-Host "    owner of made.txt: $((Get-Acl "$session\made.txt").Owner)"
# Taken back: the person (here the administrator running this) becomes the owner of everything in the folder.
icacls "$session\made.txt" /reset | Out-Null
icacls $session /setowner $env:USERNAME /T | Out-Null
$r = As $box "icacls `"$session\made.txt`" /grant $($box.Name):F && echo third> `"$session\made.txt`""
$after = (Get-Content "$session\made.txt" -Raw).Trim()
Answer 'Once the person owns it again, it cannot' ($after -ne 'third') "file says: $after; $($r.Text)"

# ---------- network ----------
$r = As $box 'curl.exe -s -o NUL -m 10 -w "%{http_code}" https://example.com'
Answer 'Before any rule, it reaches the internet' ($r.Text -match '200') $r.Text
$listener = Start-Job { $l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 47321); $l.Start(); while ($true) { $c = $l.AcceptTcpClient(); $s = $c.GetStream(); $b = [Text.Encoding]::ASCII.GetBytes("HTTP/1.0 200 OK`r`nContent-Length: 2`r`n`r`nok"); $s.Write($b, 0, $b.Length); $c.Close() } }
Start-Sleep 2
$r = As $box 'curl.exe -s -m 5 http://127.0.0.1:47321/'
Answer 'Before any rule, it reaches a server on this computer (loopback)' ($r.Text -match 'ok') $r.Text

Set-NetFirewallProfile -All -Enabled True
$sddl = "O:LSD:(A;;CC;;;$($box.Sid))"
New-NetFirewallRule -Name 'DemidoTrialOut' -DisplayName 'Demido trial: no network out' -Direction Outbound -Action Block -LocalUser $sddl -Profile Any | Out-Null
New-NetFirewallRule -Name 'DemidoTrialIn' -DisplayName 'Demido trial: no network in' -Direction Inbound -Action Block -LocalUser $sddl -Profile Any | Out-Null
Start-Sleep 2
$r = As $box 'curl.exe -s -o NUL -m 10 -w "%{http_code}" https://example.com'
Answer 'With a block rule for its user, it cannot reach the internet' ($r.Text -notmatch '200') "$($r.Text) (exit $($r.Code))"
$mine = (& curl.exe -s -o NUL -m 10 -w '%{http_code}' https://example.com)
Answer 'The person still can' ($mine -match '200') $mine
$r = As $box 'curl.exe -s -m 5 http://127.0.0.1:47321/'
Answer 'With the rule, it cannot reach a server on this computer either' ($r.Text -notmatch 'ok') "$($r.Text) (exit $($r.Code))"
$r = As $box 'powershell -NoProfile -Command "[Net.Dns]::GetHostAddresses(''example.org'') | Select-Object -First 1"'
Answer 'It can still have a name looked up (the system''s DNS service asks for it)' ($r.Text -match '\d+\.\d+\.\d+\.\d+|:') $r.Text
Stop-Job $listener; Remove-Job $listener -Force

# ---------- a person who is not an administrator ----------
# Demido runs as the person, without administrator rights. Can such a user start a program as the sandbox user?
$person = New-TrialUser 'DemidoPerson'
$hand = Join-Path $root 'person'
New-Item -ItemType Directory -Force $hand | Out-Null
icacls $hand /inheritance:r /grant "$($person.Name):(OI)(CI)F" /grant 'Administrators:(OI)(CI)F' | Out-Null
Set-Content "$hand\key.txt" $box.Plain
@"
`$secure = ConvertTo-SecureString (Get-Content '$hand\key.txt' -Raw).Trim() -AsPlainText -Force
`$cred = New-Object Management.Automation.PSCredential('$($box.Name)', `$secure)
`$p = Start-Process cmd.exe -ArgumentList '/D','/C','whoami' -Credential `$cred -WorkingDirectory '$work' -RedirectStandardOutput '$hand\inner.txt' -NoNewWindow -Wait -PassThru
'started by: ' + [Security.Principal.WindowsIdentity]::GetCurrent().Name + '; it ran as: ' + (Get-Content '$hand\inner.txt' -Raw).Trim()
"@ | Set-Content "$hand\inner.ps1"
$r = As $person "powershell -NoProfile -ExecutionPolicy Bypass -File `"$hand\inner.ps1`"" $hand
Answer 'A person without administrator rights can start a program as the sandbox user' ($r.Text -match 'it ran as: .*demidotrial') $r.Text
