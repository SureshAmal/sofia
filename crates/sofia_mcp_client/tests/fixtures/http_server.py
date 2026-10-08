import json
from http.server import BaseHTTPRequestHandler, HTTPServer
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self): self.send_error(405)
    def do_DELETE(self): self.send_response(200);self.end_headers()
    def do_POST(self):
        if self.headers.get('Authorization') != 'Bearer fixture-token': self.send_error(401);return
        request=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if 'id' not in request:
            self.send_response(202);self.end_headers();return
        method=request['method']
        if method=='initialize': result={'protocolVersion':request['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'http-fixture','version':'1'}}
        elif method=='tools/list': result={'tools':[{'name':'echo','inputSchema':{'type':'object','properties':{'text':{'type':'string'}}}}]}
        elif method=='tools/call': result={'content':[{'type':'text','text':request['params']['arguments']['text']}],'isError':False}
        else: result={}
        data=json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}).encode()
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=HTTPServer(('127.0.0.1',0),Handler)
print(server.server_port,flush=True)
server.serve_forever()
