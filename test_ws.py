import asyncio
import websockets

async def test():
    print("Connecting to ws://127.0.0.1:49184")
    async with websockets.connect("ws://127.0.0.1:49184") as ws:
        print("Connected")
        payload = bytearray(42)
        payload[0] = 2
        await ws.send(payload)
        print("Sent 42 bytes handshake")
        
        while True:
            try:
                res = await asyncio.wait_for(ws.recv(), timeout=2.0)
                if len(res) > 0:
                    print(f"Received {len(res)} bytes, type {res[0]}")
                    if res[0] == 13:
                        print("Got VIDEO PACKET!")
                        break
            except asyncio.TimeoutError:
                print("Timeout waiting for packet")
                break

asyncio.run(test())
