#!/usr/bin/env python3
import argparse, importlib.util, json, pathlib, time

base_path=pathlib.Path(__file__).resolve().parents[1]/"webgpu-cga-inversion"/"cdp_acceptance.py"
spec=importlib.util.spec_from_file_location("cdp_base",base_path); base=importlib.util.module_from_spec(spec); spec.loader.exec_module(base)

def valid(value,mode):
    if not isinstance(value,dict) or value.get("presentation") != ("canvas" if mode=="off" else "offscreen"): return False
    c=value.get("counters",{})
    if mode=="off":
        return value.get("state")=="presentation" and value.get("verified") is False and c=={"fetches":["gen/layout.json","gen/frag.wgsl","gen/kernel.fe"],"workerCreates":0,"readbacks":0} and "wasmHash" not in value and "gpuHash" not in value
    return value.get("state")=="green" and value.get("verified") is True and value.get("pixels")==16384 and value.get("wasmHash")==value.get("gpuHash")==2368784280 and isinstance(value.get("oracleMs"),(int,float)) and value["oracleMs"]>=0 and c.get("workerCreates")==1 and c.get("readbacks")==1 and "gen/actor-canonical.wasm" in c.get("fetches",[])

def main():
    p=argparse.ArgumentParser();p.add_argument("--debug-port",type=int,required=True);p.add_argument("--url",required=True);p.add_argument("--mode",choices=["verify","off"],required=True);p.add_argument("--timeout",type=float,default=90);a=p.parse_args();deadline=time.monotonic()+a.timeout
    ws=base.WebSocket(base.find_page(a.debug_port,a.url,deadline),max(1,a.timeout));i=0
    try:
        while time.monotonic()<deadline:
            i+=1;ws.send_json({"id":i,"method":"Runtime.evaluate","params":{"expression":"JSON.stringify(window.__qcgaAcceptance || null)","returnByValue":True}})
            while time.monotonic()<deadline:
                r=ws.recv_json()
                if r.get("id")!=i:continue
                raw=r.get("result",{}).get("result",{}).get("value","null");v=json.loads(raw)
                if isinstance(v,dict) and v.get("state")!="pending": print(json.dumps(v,sort_keys=True));raise SystemExit(0 if valid(v,a.mode) else 1)
                break
            time.sleep(.1)
    finally: ws.close()
    raise SystemExit("QCGA acceptance remained pending")
if __name__=="__main__":main()
