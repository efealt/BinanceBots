import {sum,histogram,activity,extent,weightedSpread} from './analytics.js';
import {plot,colors,number,utc} from './plot.js';
const $=id=>document.getElementById(id);
let payload=null, catalog=[], request=0, start=0,end=0;
document.documentElement.dataset.theme=localStorage.getItem('theme')??'dark';
$('theme-toggle').onclick=()=>{const t=document.documentElement.dataset.theme==='dark'?'light':'dark';document.documentElement.dataset.theme=t;localStorage.setItem('theme',t);};
async function get(url){const r=await fetch(url);if(!r.ok)throw new Error(await r.text());return r.json();}
function message(e){$('coverage').textContent='Unable to load diagnostics: '+e.message;$('charts').replaceChildren();$('metrics').replaceChildren();}
function card(title,description,config,wide=false){
  const section=document.createElement('section');section.className='panel'+(wide?' wide':'');
  const h=document.createElement('h2');h.textContent=title;
  const p=document.createElement('p');p.textContent=description;
  const host=document.createElement('div');section.append(h,p,host);$('charts').append(section);plot(host,config);
}
function metric(label,value){const d=document.createElement('div');d.className='panel';const s=document.createElement('small');s.textContent=label;const b=document.createElement('strong');b.textContent=value;d.append(s,b);$('metrics').append(d);}
const series=(name,color,points,kind='line')=>({name,color,points,kind});
const point=(rows,fn)=>rows.map(r=>({x:r.received_at_ms,y:fn(r)}));
async function catalogLoad(){
 const id=++request;payload=null;$('charts').replaceChildren();$('metrics').replaceChildren();$('coverage').textContent='Loading…';$('dataset').replaceChildren();
 try{
  const ws=$('source').value==='websocket';const data=await get(ws?'/api/data/captures':'/api/data/datasets');if(id!==request)return;
  catalog=ws?data.captures:data.datasets;
  for(const d of catalog){const o=document.createElement('option');o.value=ws?d.capture_id:d.dataset_id;o.textContent=`${d.symbol} · ${d.market_type} · ${new Date(d.started_at_ms??d.start_time_ms).toISOString().replace('T',' ').slice(0,19)} UTC`;$('dataset').append(o);}
  if(!catalog.length){$('coverage').textContent='No saved datasets for this source.';$('scope-note').textContent='';return;}
  await load();
 }catch(e){if(id===request)message(e);}
}
async function load(){
 const id=++request;payload=null;$('coverage').textContent='Loading all observations…';$('charts').replaceChildren();$('metrics').replaceChildren();
 try{
 const ws=$('source').value==='websocket',value=$('dataset').value;
 const data=await get(ws?`/api/data/capture-inspection?capture_id=${value}&page_size=1`:`/api/data/inspection?dataset_id=${value}&page_size=500`);
 if(!ws){
   const rows=[...data.candle_rows];
   for(let p=2;p<=data.candle_page.total_pages;p++){if(id!==request)return;const next=await get(`/api/data/inspection?dataset_id=${value}&page_size=500&page=${p}`);rows.push(...next.candle_rows);}
   data.allCandles=rows.sort((a,b)=>a.open_time_ms-b.open_time_ms);
 }
 if(id!==request)return;payload=data;
 const times=ws?[...data.tick_quotes,...data.tick_trades,...(data.depth_history??[])].map(r=>r.received_at_ms):data.allCandles.map(r=>r.open_time_ms);
 [start,end]=extent(times);
 $('from').value=0;$('to').value=(end-start)/1000;
 $('coverage').textContent=`${data.summary.symbol} · ${data.summary.market_type} · ${new Date(start).toISOString()} → ${new Date(end).toISOString()} · ${number((end-start)/1000)} seconds · all saved observations loaded`;
 render();
 }catch(e){if(id===request)message(e);}
}
function render(){
 if(!payload)return;
 const from=Number($('from').value),to=Number($('to').value),duration=(end-start)/1000;
 if(!Number.isFinite(from)||!Number.isFinite(to)||from<0||to<from||to>duration){$('scope-note').textContent='Choose an interval within the capture; the end must follow the start.';return;}
 const a=start+from*1000,b=start+to*1000,domain=[a,b], timeConfig={time:true,domain,xLabel:'Time · UTC · local receipt clock'};
 $('charts').replaceChildren();$('metrics').replaceChildren();
 if($('source').value==='rest'){renderCandles(a,b);return;}
 const q=payload.tick_quotes.filter(r=>r.received_at_ms>=a&&r.received_at_ms<=b);
 const t=payload.tick_trades.filter(r=>r.received_at_ms>=a&&r.received_at_ms<=b);
 const depth=(payload.depth_history??[]).filter(r=>r.received_at_ms>=a&&r.received_at_ms<=b);
 const base=payload.summary.symbol.endsWith('USDT')?payload.summary.symbol.slice(0,-4):'base units';
 const quote=payload.summary.symbol.endsWith('USDT')?'USDT':'quote units';
 const buy=t.filter(r=>!r.is_buyer_maker),sell=t.filter(r=>r.is_buyer_maker);
 const volume=sum(t.map(r=>r.quantity)),buyVolume=sum(buy.map(r=>r.quantity));
 const prices=t.map(r=>r.price),range=extent(prices);
 metric('Executed price range · '+quote,prices.length?range.map(number).join(' – '):'No trades');
 metric('Executed volume · '+base,number(volume));
 metric('Aggressive buy share · volume',volume?(buyVolume/volume*100).toFixed(2)+'%':'No trades');
 const spread=weightedSpread(q,b);metric('Time-weighted spread · '+quote,spread===null?'No quote duration':number(spread));
 $('scope-note').textContent=`${t.length} trades · ${q.length} quote updates · ${depth.length} depth snapshots in the selected interval. All time plots use receipt time to align streams. Trade sides describe the aggressor. This short capture describes this interval only; it cannot establish a persistent market pattern.`;
 card('Executed price path', 'Each point is an actual trade. Equal time distances mean equal elapsed time; repeated timestamps are preserved.',{...timeConfig,yLabel:'Trade price · '+quote,series:[series('Buy executions · '+quote,colors.buy,point(buy,r=>r.price),'scatter'),series('Sell executions · '+quote,colors.sell,point(sell,r=>r.price),'scatter')]},true);
 const seconds=Math.max(Number($('bucket').value),Math.ceil((b-a)/1000/2000));
 const bins=activity(t,a,b,seconds);
 card('Who traded, and when?',`Executed volume in ${seconds}-second buckets. Buy volume above zero; sell volume below zero. Empty buckets remain visible.`,{...timeConfig,zero:true,yLabel:'Volume · '+base,series:[series('Aggressive buys · '+base,colors.buy,bins.map(r=>({x:r.x,y:r.buy})),'bar'),series('Aggressive sells · '+base,colors.sell,bins.map(r=>({x:r.x,y:-r.sell})),'bar')]});
 card('Trade-size distribution','Every execution contributes once. Equal-width bins; hover for the size range and count.',{xLabel:'Execution size · '+base,yLabel:'Number of trades',zero:true,series:[series('Trade count',colors.price,histogram(t.map(r=>r.quantity)),'bar')]});
 card('Spread through time','Best ask minus best bid. A flat line is meaningful when the spread stays constant.',{...timeConfig,yLabel:'Spread · '+quote,series:[series('Quoted spread · '+quote,colors.neutral,point(q,r=>r.ask_price-r.bid_price))]});
 card('Trading intensity',`Number of executions in each ${seconds}-second bucket. Shows bursts and quiet periods independently of price.`,{...timeConfig,zero:true,yLabel:'Trades per bucket',series:[series('Executions / '+seconds+'s',colors.price,bins.map(r=>({x:r.x,y:r.count})),'bar')]});
 let delta=0;
 card('Cumulative aggressive volume','Running buy volume minus sell volume, reset at the selected interval start. Positive means more buyer-initiated volume.',{...timeConfig,zero:true,yLabel:'Net volume · '+base,series:[series('Net aggressive volume · '+base,colors.price,point(t,r=>delta+=(r.is_buyer_maker?-1:1)*r.quantity))]});
 card('Best-quote liquidity imbalance','(Bid size − ask size) / (bid size + ask size). +1 means bid-dominated; −1 ask-dominated. Displayed liquidity is not a promise of execution.',{...timeConfig,bounds:[-1,1],yLabel:'Imbalance · ratio',series:[series('Best bid/ask size imbalance',colors.neutral,point(q.filter(r=>r.bid_quantity+r.ask_quantity>0),r=>(r.bid_quantity-r.ask_quantity)/(r.bid_quantity+r.ask_quantity)))]});
 card('Available depth through time','Sum of displayed size on each side across the captured levels. This is partial depth, not the entire order book.',{...timeConfig,zero:true,yLabel:'Displayed size · '+base,series:[series('Bid depth · '+base,colors.buy,point(depth,r=>sum(r.bids.map(l=>l.quantity)))),series('Ask depth · '+base,colors.sell,point(depth,r=>sum(r.asks.map(l=>l.quantity))))]});
 const last=depth.at(-1);
 const cumulative=side=>{let total=0;return (last?.[side]??[]).slice().sort((a,b)=>side==='bids'?b.price-a.price:a.price-b.price).map(l=>({x:l.price,y:total+=l.quantity})).sort((a,b)=>a.x-b.x);};
 card('Cumulative depth at interval end',last?`Snapshot at ${utc(last.received_at_ms)} UTC. Each curve accumulates size outward from the best quote; captured levels only.`:'No stored depth snapshots in this interval.',{xLabel:'Price · '+quote,yLabel:'Cumulative size · '+base,zero:true,series:[series('Bid liquidity · '+base,colors.buy,cumulative('bids')),series('Ask liquidity · '+base,colors.sell,cumulative('asks'))]});
 const gaps=t.slice(1).map((r,i)=>r.received_at_ms-t[i].received_at_ms);
 card('Time between received trades','Distribution of successive receipt-time gaps in milliseconds. Includes zero when trades arrive within the same millisecond; gaps alone do not prove data loss.',{xLabel:'Inter-arrival gap · milliseconds',yLabel:'Number of intervals',zero:true,series:[series('Intervals',colors.neutral,histogram(gaps),'bar')]});
 const spreads=q.map(r=>r.ask_price-r.bid_price);
 card('Spread distribution','Quote-update-weighted distribution: each saved quote update contributes once. This is not the fraction of time spent at each spread.',{xLabel:'Spread · '+quote,yLabel:'Quote update count',zero:true,series:[series('Quote updates',colors.neutral,histogram(spreads),'bar')]});
}
function renderCandles(a,b){
 const rows=payload.allCandles.filter(r=>r.open_time_ms>=a&&r.open_time_ms<=b);
 const map=key=>rows.map(r=>({x:r.open_time_ms,y:r[key]}));
 const config={time:true,domain:[a,b],xLabel:'Candle open · UTC'};
 $('scope-note').textContent=`${rows.length} stored candles · ${payload.summary.interval}. Candle data cannot reveal intrabar event order or historical bid/ask liquidity.`;
 metric('Stored candles',number(rows.length));metric('Base volume',number(sum(rows.map(r=>r.base_volume))));
 const returns=rows.slice(1).map((r,i)=>100*(r.close_price/rows[i].close_price-1));
 metric('Observed low',rows.length?number(extent(rows.map(r=>r.low_price))[0]):'—');metric('Observed high',rows.length?number(extent(rows.map(r=>r.high_price))[1]):'—');
 card('Closing price', 'Every stored candle close in the selected interval.',{...config,yLabel:'Price · quote units',series:[series('Close',colors.price,map('close_price'))]},true);
 card('Traded volume','Base volume for each candle.',{...config,zero:true,yLabel:'Base volume',series:[series('Base volume',colors.buy,map('base_volume'),'bar')]});
 card('Return distribution','Close-to-close percentage changes across consecutive stored candles; inspect coverage before interpreting gaps.',{xLabel:'Close return · %',yLabel:'Candle count',zero:true,series:[series('Candles',colors.price,histogram(returns),'bar')]});
}
$('source').onchange=catalogLoad;$('dataset').onchange=load;$('bucket').onchange=render;$('apply').onclick=render;
$('reset').onclick=()=>{$('from').value=0;$('to').value=(end-start)/1000;render();};
catalogLoad();
