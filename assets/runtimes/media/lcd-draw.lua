local z=vsn1_cli_state
local t=os.clock()local l,p,e=z.b,z.l,z.u
for i=1,#z.o do local n=z.o[i]if(e[n]or 0)>t then l=n end end
if z.r==0 and z.w==l then return end
z.r=0 z.w=l
local q=p.base
local d,g=q.d,0 if d>0 then g=q.p/d end if g<0 then g=0 elseif g>1 then g=1 end
self:ldaf(0,0,319,239,c[1])self:ldft(q.a,18,18,12,c[4])self:ldft(q.t,18,52,16,c[2])self:ldft(q.l,18,86,12,c[5])self:ldrr(18,178,301,202,8,c[2])local x=18+math.floor(283*g)if x>18 then self:ldrrf(19,179,x,201,7,c[3])end self:ldft(T(q.p),18,212,16,c[2])self:ldft(T(d),252,212,16,c[2])
if l=="playback_status" then local s=p.playback_status.s if s~="" then self:ldaf(0,0,319,239,c[1])self:ldft(s,18,111,24,c[2])end end
self:ldsw()
