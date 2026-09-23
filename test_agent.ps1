$agentPath = "C:\xampp\htdocs\Screen Share\desktop-agent\target\release\desktop-agent.exe"
$logFile = "C:\Users\Public\DeskStream-direct.log"
$finalVerification = "C:\Users\Public\DeskStream-final-verification.txt"

Write-Host "Starting agent..."
Start-Process -FilePath $agentPath -RedirectStandardOutput $logFile -RedirectStandardError $logFile -WindowStyle Hidden

Start-Sleep -Seconds 2

Write-Host "Connecting to ws://127.0.0.1:49184 to simulate handshake..."

$code = @"
using System;
using System.Net.WebSockets;
using System.Threading;
using System.Threading.Tasks;
using System.Text;

public class WsTest {
    public static async Task Run() {
        using (var ws = new ClientWebSocket()) {
            await ws.ConnectAsync(new Uri("ws://127.0.0.1:49184"), CancellationToken.None);
            Console.WriteLine("Connected");

            byte[] payload = new byte[42];
            payload[0] = 2; // Type 2
            byte[] id = Encoding.UTF8.GetBytes("test-id-1");
            Array.Copy(id, 0, payload, 1, id.Length);

            await ws.SendAsync(new ArraySegment<byte>(payload), WebSocketMessageType.Binary, true, CancellationToken.None);
            Console.WriteLine("Sent 42 bytes handshake");

            byte[] buffer = new byte[1024 * 1024];
            var result = await ws.ReceiveAsync(new ArraySegment<byte>(buffer), CancellationToken.None);
            
            if (result.Count > 0 && buffer[0] == 2) {
                Console.WriteLine("Auth Response [2] received!");
            }
            
            // Wait for video packet
            for(int i=0; i<5; i++) {
                var vres = await ws.ReceiveAsync(new ArraySegment<byte>(buffer), CancellationToken.None);
                if (vres.Count > 0 && buffer[0] == 13) {
                    Console.WriteLine("Video packet (Type 13) received! Size: " + vres.Count);
                    break;
                }
            }
        }
    }
}
"@

Add-Type -TypeDefinition $code -Language CSharp
[WsTest]::Run().Wait()

Write-Host "Stopping agent..."
Stop-Process -Name "desktop-agent" -Force

Get-Content $logFile | Out-File $finalVerification

Write-Host "Done"
