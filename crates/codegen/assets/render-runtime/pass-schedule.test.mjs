import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
globalThis.HTMLElement = class {};
globalThis.customElements = {define() {}};
const {planPassSchedule,FeSurfaceElement} = await import('./fe-render-runtime.js');
const cycle = (group, repeat, inner) => ({group, repeat, inner});
const record = (name, cycle) => ({pass: {source_entry:name, cycle, layout:{mode:'compute'}}});

test('recorded Chromium execution matches the Fe fixture ordering receipt',()=>{
  const data=JSON.parse(readFileSync(new URL('../../tests/fixtures/actor_nested_cycles/browser-evidence.json',import.meta.url),'utf8'));
  assert.equal(data.passCount,6);
  assert.equal(data.deviceStorageBindingLimit,8);
  assert.deepEqual(data.receipt,[17,1,2,3,2,3,2,3,4,1,2,3,2,3,2,3,4,5,...Array(14).fill(0)]);
});

test('nested cycles retain one shared body and reset inner iterations per job', () => {
  const outer = cycle(0,2), nested = cycle(0,2,cycle(1,3));
  const plan = planPassSchedule([record('begin',outer),record('a',nested),record('b',nested),record('end',outer),record('finish')]);
  assert.equal(plan.length,2);
  assert.equal(plan[0].body.length,3);
  assert.equal(plan[0].body[1].body.length,2);
  const actual=[];
  const run=(nodes, iteration=null)=>{
    for(const node of nodes) {
      if(node.record) actual.push([node.record.pass.source_entry,iteration]);
      else for(let i=0;i<node.repeat;i++) run(node.body,i);
    }
  };
  run(plan);
  assert.deepEqual(actual,[
    ['begin',0],['a',0],['b',0],['a',1],['b',1],['a',2],['b',2],['end',0],
    ['begin',1],['a',0],['b',0],['a',1],['b',1],['a',2],['b',2],['end',1],['finish',null],
  ]);
});
test('large loop counts do not expand the command plan',()=>{
  const plan=planPassSchedule([record('work',cycle(0,65535,cycle(1,65535)))]);
  assert.equal(plan[0].body[0].body.length,1);
});
for(const [name, cycles] of [
  ['self nesting',[cycle(0,2,cycle(0,3))]],
  ['different inner counts',[cycle(0,2,cycle(1,2)),cycle(0,2,cycle(1,3))]],
  ['reopened inner group',[cycle(0,2,cycle(1,2)),cycle(0,2),cycle(0,2,cycle(1,2))]],
  ['reparented inner group',[cycle(0,2,cycle(1,2)),cycle(2,2,cycle(1,2))]],
  ['zero count',[cycle(0,0)]],
]) test(`rejects ${name}`,()=>assert.throws(()=>planPassSchedule(cycles.map(c=>record('work',c)))));

test('actual host dispatch preserves nested order and resets tapered work',async()=>{
  const trace=[];
  const device={
    createCommandEncoder(){
      const commands=[]; let pipeline;
      return {
        beginComputePass(){return {
          setPipeline(p){pipeline=p;},setBindGroup(){},end(){},
          dispatchWorkgroups(x){commands.push([pipeline.name,x]);},
        };},
        finish(){return commands;},
      };
    },
    queue:{submit(buffers){trace.push(...buffers.flat());}},
  };
  const make=(name,c,taper)=>({
    ...record(name,c),pipeline:{name},bindGroup:null,inputs:[],
    pass:{...record(name,c).pass,dispatch:[8,1,1],repeat:taper?3:1,taper},
  });
  const outer=cycle(0,2),nested=cycle(0,2,cycle(1,3));
  const surface=Object.create(FeSurfaceElement.prototype);
  surface._graph=true; surface._memberIndexByName=new Map();
  surface._gpu={device,generation:1,passRecords:[
    make('begin',outer),make('inner',nested,{shifts:[1,0,0],repeat_decrement:1}),make('end',outer),
  ]};
  await surface._presentOn({},[]);
  const job=[['begin',8],['inner',8],['inner',8],['inner',8],['inner',4],['inner',4],['inner',2],['end',8]];
  assert.deepEqual(trace,[...job,...job]);
});
