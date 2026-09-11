#!/usr/bin/env python3
"""Read-only queries recovered from Ghidra; saves raw replies for reproducibility."""
import json, os, select, time
from pathlib import Path

def packet(command, length=0, offset=0):
    data = bytearray(64)
    data[0], data[3], data[4] = 4, command, length
    data[5:7] = offset.to_bytes(2, 'little')
    data[1:3] = sum(data[3:]).to_bytes(2, 'little')
    return data

if __name__ == '__main__':
    candidates = [p for p in Path('/sys/class/hidraw').iterdir()
                  if '0000320F:0000505B' in (p/'device/uevent').read_text()
                  and b'\x06\x1c\xff\x09\x92' in (p/'device/report_descriptor').read_bytes()]
    if len(candidates) != 1: raise RuntimeError('Expected exactly one Phantom vendor interface')
    fd = os.open('/dev/'+candidates[0].name, os.O_RDWR | os.O_NONBLOCK)
    log=[]
    def query(command,length=0,offset=0):
        tx=packet(command,length,offset); os.write(fd,tx)
        deadline=time.monotonic()+2
        while time.monotonic()<deadline:
            if select.select([fd],[],[],0.1)[0]:
                rx=os.read(fd,64)
                if len(rx)==64 and rx[0]==4 and rx[3]==command:
                    log.append({'tx':tx.hex(), 'rx':rx.hex()})
                    print(hex(command),offset,rx.hex(' '));return rx[8:8+length]
        raise TimeoutError(hex(command))
    try:
        query(1)
        info=query(3,34)
        settings=query(5,26)
        keys=b''.join(query(7,min(56,info[5]*3-i),i) for i in range(0,info[5]*3,56))
        leds=b''.join(query(0x1b,min(56,info[5]-i),i) for i in range(0,info[5],56))
        Path('phantom-linux/research/device-backup.json').write_text(json.dumps({'info':list(info),'settings':list(settings),'keys':list(keys),'leds':list(leds),'transactions':log},indent=2))
    finally:
        query(2);os.close(fd)
