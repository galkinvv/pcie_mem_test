# pcie_mem_test
linux debug tool for directly writing to PCIe bar memory (actually any memory area that can be accessed by mmapping a file).

Use with caution! If you accidently pass storage device memory BAR as an argument - data will be destroyed.

The main use case is testing NVIDIA & AMD gpus memory with driver unloaded. The gpu instabce is selected by specifying PCIe device like `0000:01:00.0`

For AMD GCN GPUs memory is typically mapped as BAR0. Example with amd GPU with display connected to integrated intel, so driver unloading is possible and nothing touch the external gpu memory:
<pre>
[user@host ~]$ sudo rmmod amdgpu
[user@user ~]$ lspci -vv -d ::0300
00:02.0 VGA compatible controller: Intel Corporation HD Graphics 610 (rev 04) (prog-if 00 [VGA controller])
        DeviceName:  Onboard IGD
        Subsystem: ASUSTeK Computer Inc. Device 8694
        Control: I/O+ Mem+ BusMaster+ SpecCycle- MemWINV- VGASnoop- ParErr- Stepping- SERR- FastB2B- DisINTx-
        Status: Cap+ 66MHz- UDF- FastB2B- ParErr- DEVSEL=fast TAbort- TAbort- MAbort- SERR- PERR- INTx-
        Latency: 0
        Interrupt: pin A routed to IRQ 11
        Region 0: Memory at de000000 (64-bit, non-prefetchable) [size=16M]
        Region 2: Memory at b0000000 (64-bit, prefetchable) [size=256M]
        Region 4: I/O ports at f000 [size=64]
        Expansion ROM at 000c0000 [virtual] [disabled] [size=128K]
        Capabilities: access denied
        Kernel modules: i915

<b>01:00.0</b> VGA compatible controller: Advanced Micro Devices, Inc. [AMD/ATI] Ellesmere [Radeon RX 470/480/570/570X/580/580X/590] (rev e7) (prog-if 00 [VGA controller])
        Subsystem: Gigabyte Technology Co., Ltd Device 22fc
        Control: I/O+ Mem+ BusMaster- SpecCycle- MemWINV- VGASnoop- ParErr- Stepping- SERR- FastB2B- DisINTx-
        Status: Cap+ 66MHz- UDF- FastB2B- ParErr- DEVSEL=fast TAbort- TAbort- MAbort- SERR- PERR- INTx-
        Interrupt: pin A routed to IRQ 16
        <b>Region 0</b>: Memory at c0000000 (64-bit, prefetchable) [<b>size=256M</b>]
        Region 2: Memory at d0000000 (64-bit, prefetchable) [size=2M]
        Region 4: I/O ports at e000 [size=256]
        Region 5: Memory at df500000 (32-bit, non-prefetchable) [size=256K]
        Expansion ROM at df540000 [disabled] [size=128K]
        Capabilities: access denied
        Kernel modules: amdgpu

[user@host ~]$ sudo ./pcie_mem_test /sys/bus/pci/devices/<b>0000:01:00.0</b>/resource<b>0</b>
usage: boot with console=null and run as root passing path with a pcie memory bar mapping as a single argument. By https://github.com/galkinvv/pcie_mem_test  Typical example: ./pcie_mem_test /sys/bus/pci/devices/0000:01:00.0/resource1
Testing "/sys/bus/pci/devices/0000:01:00.0/resource0", size 268435456=0x10000000 ... First pass done without miscompares in 13978 milliseconds, all 3 iterations are expected to be done in 41 seconds... PASS: iterations 3
[user@host ~]$ 
</pre>
In the absence of another gpu - boot with console=null kernel parameter and run test via ssh access.


NVIDIA memory is typically mapped as BAR1. So, typical usage for NVIDIA
```
./pcie_mem_test /sys/bus/pci/devices/0000:01:00.0/resource1
```



If any errors are detected - the output contains lines like this showing the address, expected value (upper) an actual value from memory (lower):
```
0x0267b820: 4820267b 167b8242 528267b8 27b82c26   60267b83 3b834267 0267b838 483c267b 
     MEMBAR 0820267b 167b8242 528267b8 27b82c26   60267b83 3b834267 0267b838 483c267b FAIL 
```

There is a known bug - if the memory erros are not stable - sometimes failed line in identical to expected, since the second reread gives desired value,
adding something like volatile/atomic access may be useful.
