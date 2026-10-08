import json, sys
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'initialize':
        result = {'protocolVersion': request['params']['protocolVersion'], 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'tools/list':
        cursor = request.get('params', {}).get('cursor')
        result = {'tools': [{'name': 'second' if cursor else 'echo', 'description': 'Echo supplied text', 'inputSchema': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]}
        if not cursor:
            result['nextCursor'] = 'page2'
    elif method == 'tools/call':
        result = {'content': [{'type': 'text', 'text': request['params']['arguments']['text']}], 'isError': False}
    elif method == 'ping':
        result = {}
    else:
        print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'error': {'code': -32601, 'message': 'Unknown method'}}), flush=True)
        continue
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
