"""Measure actual hotkey-to-window latency and cancellation on the selected live game."""
import ctypes
import json
import time
from pathlib import Path
import argparse
import pyautogui
import mss
pyautogui.PAUSE=0

parser=argparse.ArgumentParser()
parser.add_argument('--pid',type=int,required=True)
args=parser.parse_args()
user=ctypes.WinDLL('user32')
user.FindWindowW.argtypes=[ctypes.c_wchar_p,ctypes.c_wchar_p]
user.FindWindowW.restype=ctypes.c_void_p
user.IsWindowVisible.argtypes=[ctypes.c_void_p]
overlay=user.FindWindowW(None,'Translit Overlay')
if not overlay:raise RuntimeError('Overlay window is not running')
if user.IsWindowVisible(overlay):raise RuntimeError('Resume the existing overlay before measuring')
directory=Path(__import__('os').environ['TEMP'])/f'translit-native-v2-{args.pid}'
artifact=Path(__file__).resolve().parent.parent/'artifacts'
started=time.perf_counter()
pyautogui.keyDown('f8');time.sleep(.06);pyautogui.keyUp('f8')
for _ in range(500):
    if user.IsWindowVisible(overlay):break
    time.sleep(.01)
else:raise RuntimeError('Overlay did not appear')
visible=time.perf_counter()-started
time.sleep(.1)
with mss.MSS() as screen:
    shot=screen.grab({'left':0,'top':0,'width':3440,'height':1440})
    mss.tools.to_png(shot.rgb,shot.size,output=str(artifact/'overlay-first-paint.png'))
cancelled=time.perf_counter()
pyautogui.press('esc')
for _ in range(300):
    if not user.IsWindowVisible(overlay) and not (directory/'paused.status').exists():break
    time.sleep(.01)
else:raise RuntimeError('Escape did not release the pause')
result={'game_pid':args.pid,'hotkey_to_visible_seconds':visible,'escape_to_resume_seconds':time.perf_counter()-cancelled}
(artifact/'overlay-timing.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps(result))
