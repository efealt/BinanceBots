import assert from 'node:assert/strict';
import {histogram,activity,sum,weightedSpread} from './analytics.js';
const trades=[{received_at_ms:0,quantity:2,is_buyer_maker:false},{received_at_ms:0,quantity:3,is_buyer_maker:true},{received_at_ms:2000,quantity:4,is_buyer_maker:false}];
const bins=activity(trades,0,3000,1);
assert.deepEqual(bins.map(b=>b.count),[2,0,1]);
assert.equal(sum(bins.map(b=>b.buy+b.sell)),9);
assert.equal(sum(histogram([0,0,1,2,100]).map(b=>b.y)),5);
assert.equal(histogram([2,2,2])[0].y,3);
assert.deepEqual(histogram([]),[]);
assert.equal(weightedSpread([{received_at_ms:0,bid_price:10,ask_price:11},{received_at_ms:10,bid_price:10,ask_price:13}],20),2);
console.log('Diagnostic aggregation checks passed');

