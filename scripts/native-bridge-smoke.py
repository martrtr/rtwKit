# Isolated real-Host protocol acceptance; launch credentials stay in memory.
import base64,json,os,pathlib,queue,re,socket,struct,subprocess,tempfile,threading,time,signal,urllib.parse
core=pathlib.Path(os.environ['RINTAWA_BINARY'])
artifacts=pathlib.Path(os.environ['RTWKIT_SMOKE_ARTIFACTS'])
root=pathlib.Path(tempfile.mkdtemp(prefix='rtwkit-p10-smoke-'));home=root/'home'
def cmd(*args):
 p=subprocess.run([str(core),'--home',str(home),*map(str,args)],text=True,capture_output=True)
 if p.returncode:raise RuntimeError(p.stderr+p.stdout)
 return p.stdout

def install(path,component,permissions):
 output=cmd('install',path);instance=re.search(r'^instance = (.+)$',output,re.M)[1]
 for permission in permissions:cmd('grant-runtime',instance,component,permission)
 return instance
install(artifacts/'web-runtime.rtw','runtime',['background-task','loopback-listen'])
install(artifacts/'web-ui.rtw','shell',['background-task','world-session-read','world-session-write'])
install(artifacts/'dice.rtw','dice',['background-task'])
process=subprocess.Popen([str(core),'--home',str(home),'run'],env=os.environ|{'RUST_LOG':'info'},stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,bufsize=1)
lines=queue.Queue();log=(root/'host.log').open('w')
def output():
 for line in process.stdout:
  lines.put(line);log.write(re.sub(r'#[A-Za-z0-9_-]{43}', '#<redacted>',line));log.flush()
threading.Thread(target=output,daemon=True).start()
try:
 deadline=time.monotonic()+20;url=None
 while time.monotonic()<deadline:
  try:line=lines.get(timeout=.2)
  except queue.Empty:
   if process.poll() is not None:raise RuntimeError('Host exited; inspect redacted host log')
   continue
  match=re.search(r'http://127\.0\.0\.1:\d+/shell/[^\s\x1b]+',line)
  if match:url=match[0];break
 assert url,'No private launch URL; inspect redacted host log'
 parts=urllib.parse.urlsplit(url);port=parts.port;token=parts.fragment
 class WS:
  def __init__(self,origin=None):
   self.sock=socket.create_connection(('127.0.0.1',port),timeout=4);self.buffer=b''
   key=base64.b64encode(os.urandom(16)).decode()
   req=f'GET /bridge HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin or "http://127.0.0.1:"+str(port)}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n\r\n'
   self.sock.sendall(req.encode());head=b''
   while b'\r\n\r\n' not in head:
    piece=self.sock.recv(65536)
    assert piece,'Upgrade closed'
    head+=piece
   self.header,self.buffer=head.split(b'\r\n\r\n',1)
  def send(self,value):
   payload=json.dumps(value,separators=(',',':')).encode();mask=os.urandom(4);n=len(payload)
   header=bytes([0x81,0x80|n]) if n<126 else bytes([0x81,0xfe])+struct.pack('!H',n) if n<=65535 else bytes([0x81,0xff])+struct.pack('!Q',n)
   self.sock.sendall(header+mask+bytes(byte^mask[i%4] for i,byte in enumerate(payload)))
  def exact(self,n):
   while len(self.buffer)<n:
    data=self.sock.recv(65536)
    if not data:raise EOFError('connection closed')
    self.buffer+=data
   result,self.buffer=self.buffer[:n],self.buffer[n:];return result
  def receive(self):
   first,second=self.exact(2);n=second&127
   if n==126:n=struct.unpack('!H',self.exact(2))[0]
   if n==127:n=struct.unpack('!Q',self.exact(8))[0]
   assert first&15==1,(first,n)
   return json.loads(self.exact(n))
  def call(self,channel,method,args):
   self.send({'v':1,'id':'r','channel':channel,'method':method,'args':args});return self.receive()
 bad=WS('null');assert b'403' in bad.header;bad.sock.close()
 ws=WS();assert b'101' in ws.header
 ws.send({'v':1,'type':'auth','token':token});ready=ws.receive();assert ready['type']=='ready';shell=ready['channel']
 listing=ws.call(shell,'ui.list',{});assert listing['ok'];views=[v for v in listing['value'] if v['restoreKey']['componentId']=='dice'];assert len(views)==1
 a=ws.call(shell,'ui.mount',{'id':views[0]['id']})['value']['channel'];b=ws.call(shell,'ui.mount',{'id':views[0]['id']})['value']['channel']
 assert not ws.call(a,'world-sessions.list-worlds',{})['ok']
 declaration=ws.call(a,'transport.declare-polled',{'point':'fixture/browser'})['value']
 foreign=ws.call(b,'transport.close-queryable',{'handle':declaration});assert foreign['error']['code']=='invalid-handle'
 outgoing=ws.call(b,'transport.begin-query',{'point':'fixture/browser','payload':base64.b64encode(b'hello').decode()})['value']
 request=None
 for _ in range(10):
  request=ws.call(a,'transport.next-query',{'queryable':declaration})['value']
  if request:break
 assert request and base64.b64decode(request['payload'])==b'hello'
 assert request['caller']['componentId']=='dice' and isinstance(request['caller']['activation'],str)
 assert ws.call(a,'transport.reply-query',{'handle':request['handle'],'payload':base64.b64encode(b'answer').decode()})['ok']
 result=None
 for _ in range(20):
  result=ws.call(b,'transport.poll-query',{'handle':outgoing})
  if result.get('value') is not None:break
 assert result['ok'] and [base64.b64decode(x) for x in result['value']]==[b'answer'],result
 layout={'version':1,'panels':[{'panelId':'one','restoreKey':views[0]['restoreKey']}],'dockview':{}}
 assert ws.call(shell,'ui.layout.set',{'layout':layout})['ok']
 assert ws.call(shell,'ui.layout.get',{})['value']==layout
 assert not ws.call(a,'ui.layout.get',{})['ok']
 assert ws.call(shell,'ui.unmount',{'channel':a})['ok']
 assert ws.call(a,'transport.next-query',{'queryable':declaration})['error']['code']=='channel-closed'
 assert ws.call(shell,'ui.layout.reset',{})['ok']
 assert ws.call(shell,'ui.layout.get',{})['value'] is None
 assert ws.call(shell,'world-sessions.list-worlds',{})['ok']
 # Reauthentication preserves token, but every connection gets fresh channels.
 same=WS();same.send({'v':1,'type':'auth','token':ready['token']});same_ready=same.receive()
 assert same_ready['token']==ready['token'] and same_ready['channel']!=shell
 assert same.call(b,'transport.begin-query',{'point':'fixture/browser','payload':''})['error']['code']=='channel-closed'
 # A renewed single-use operator link can establish a distinct session.
 renewed=None;deadline=time.monotonic()+5
 while time.monotonic()<deadline and renewed is None:
  try:line=lines.get(timeout=.2)
  except queue.Empty:continue
  match=re.search(r'http://127\.0\.0\.1:\d+/shell/[^\s\x1b]+',line)
  if match:renewed=urllib.parse.urlsplit(match[0]).fragment
 assert renewed,'Replacement bootstrap was not issued'
 other=WS();other.send({'v':1,'type':'auth','token':renewed});other_ready=other.receive()
 assert other_ready['token']!=ready['token']
 assert ws.call(shell,'session.close',{})['ok']
 assert other.call(other_ready['channel'],'world-sessions.list-worlds',{})['ok']
 # Logout revoked the sibling connection, without closing the other session.
 try:
  same.call(same_ready['channel'],'ui.list',{})
  raise AssertionError('Sibling session connection survived logout')
 except (EOFError,ConnectionError):pass
 except AssertionError as error:
  # RFC close frame is also an explicit connection revocation.
  assert len(error.args)==1 and isinstance(error.args[0],tuple) and error.args[0][0]&15==8, error
 other.sock.close();same.sock.close()
 ws.sock.close()
 print(json.dumps({'result':'passed','checks':['real WASM runtime target','exact Origin rejection','extension offer without negotiation','same-activation channel resource isolation','D8 query/reply without blocked pump','attested caller','shell-owned layout operations','panel admin denial','unmount cleanup','same-token reconnect uses fresh channels','logout revokes sibling connections and preserves another session'],'home':str(home),'redacted_host_log':str(log.name)}))
finally:
 if process.poll() is None:
  process.send_signal(signal.SIGINT)
  try:process.wait(timeout=8)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 log.close()
