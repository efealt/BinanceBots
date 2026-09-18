export const sum = xs => xs.reduce((a,b)=>a+b,0);
export function histogram(values, count=20) {
  if (!values.length) return [];
  const [lo,hi]=extent(values);
  if(lo===hi) return [{x:lo,y:values.length,label:String(lo)}];
  const width=(hi-lo)/count;
  const bins=Array.from({length:count},(_,i)=>({x:lo+(i+.5)*width,y:0,label:`${(lo+i*width).toPrecision(4)} – ${(lo+(i+1)*width).toPrecision(4)}`}));
  values.forEach(v=>bins[Math.min(count-1,Math.floor((v-lo)/width))].y++);
  return bins;
}
export function activity(trades,start,end,seconds) {
  const width=seconds*1000;
  const bins=Array.from({length:Math.max(1,Math.ceil((end-start)/width))},(_,i)=>({x:start+(i+.5)*width,buy:0,sell:0,count:0}));
  for(const t of trades) {
    const b=bins[Math.min(bins.length-1,Math.floor((t.received_at_ms-start)/width))];
    if(!b) continue;
    b[t.is_buyer_maker?'sell':'buy']+=t.quantity; b.count++;
  }
  return bins;
}
export function extent(values) {
  let lo=Infinity,hi=-Infinity;
  for(const v of values) if(Number.isFinite(v)){lo=Math.min(lo,v);hi=Math.max(hi,v);}
  return lo===Infinity?[0,1]:[lo,hi];
}
export function weightedSpread(quotes,end) {
  let total=0,duration=0;
  for(let i=0;i<quotes.length;i++){
    const dt=Math.max(0,(quotes[i+1]?.received_at_ms??end)-quotes[i].received_at_ms);
    total+=(quotes[i].ask_price-quotes[i].bid_price)*dt; duration+=dt;
  }
  return duration?total/duration:null;
}

