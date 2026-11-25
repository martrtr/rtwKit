import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { connectPanel, BridgeError, type Client, type Theme, type Palette } from '@rintawa/bridge';
import { Button, Status } from '@rintawa/ui';
import '@rintawa/ui/style.css';
import './style.css';
const POINT = 'dice/contributions/v1';
function Dice() {
  const canvas = useRef<HTMLCanvasElement>(null); const client = useRef<Client | null>(null); const [roll, setRoll] = useState(20); const [status, setStatus] = useState('Connecting…'); const [error, setError] = useState(''); const [result, setResult] = useState('');
  const [theme, setTheme] = useState<{ theme: Theme; palette: Palette } | null>(null); const [serving, setServing] = useState(false); const service = useRef<{ close: () => Promise<void> } | null>(null);
  const serve = async () => {
    if (!client.current) return;
    setError('');
    try {
      const active = await client.current.serve(POINT, () => new TextEncoder().encode(JSON.stringify({ actions: [{ id: 'dice.roll', label: 'Roll d20' }] })));
      service.current = active; setServing(true); setStatus('Contribution ready');
      void active.done.catch(error => { setServing(false); setError(error instanceof Error ? error.message : 'Contribution unavailable'); });
    } catch (error) {
      if (error instanceof BridgeError && error.code === 'duplicate') setStatus('Another panel provides this contribution.');
      else setError(error instanceof Error ? error.message : 'Contribution unavailable');
    }
  };
  useEffect(() => {
    let disposed = false;
    connectPanel((theme, palette) => setTheme({ theme, palette })).then(bridge => {
      if (disposed) { bridge.dispose(); return; } client.current = bridge; void serve();
    }).catch(error => { if (!disposed) setError(error instanceof Error ? error.message : 'Connection unavailable'); });
    return () => { disposed = true; void service.current?.close().catch(() => {}); client.current?.dispose(); };
  }, []);
  useEffect(() => {
    const target = canvas.current; if (!target || !theme) return;
    const ctx = target.getContext('2d')!; ctx.fillStyle = theme.palette.background; ctx.fillRect(0, 0, target.width, target.height);
    ctx.strokeStyle = theme.palette.accent; ctx.lineWidth = 3; ctx.beginPath();
    for (let i = 0; i < 6; i++) { const x = 160 + 100 * Math.cos(i * Math.PI / 3 - Math.PI / 6), y = 130 + 100 * Math.sin(i * Math.PI / 3 - Math.PI / 6); if (!i) ctx.moveTo(x, y); else ctx.lineTo(x, y); } ctx.closePath(); ctx.stroke();
    ctx.fillStyle = theme.palette.text; ctx.font = '600 66px system-ui'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; ctx.fillText(String(roll), 160, 130);
  }, [roll, theme]);
  const ask = async () => { try { const replies = await client.current!.query(POINT, new TextEncoder().encode('{"kind":"menu"}')); setResult(replies.map(bytes => new TextDecoder().decode(bytes)).join('\n') || 'No contributions'); } catch (error) { setError(error instanceof Error ? error.message : 'Request unavailable'); } };
  return <main className="dice"><div className="eyebrow">Independent extension</div><h1>Dice</h1><Status>{status}</Status><canvas ref={canvas} width={320} height={260} aria-label={`d20 result: ${roll}`} /><div className="actions"><Button onClick={() => setRoll(1 + crypto.getRandomValues(new Uint32Array(1))[0] % 20)}>Roll d20</Button><Button onClick={() => void ask()}>Ask contributions</Button>{!serving && <Button onClick={() => void serve()}>Enable contribution</Button>}</div><Status>Canvas and contribution queries use this view's own World context.</Status>{result && <pre className="result">{result}</pre>}{error && <Status error>{error}</Status>}</main>;
}
createRoot(document.getElementById('root')!).render(<Dice />);
