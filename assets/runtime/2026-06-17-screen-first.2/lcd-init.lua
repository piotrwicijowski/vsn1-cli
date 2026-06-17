local function I()return{"---","---","---","---","---","---","---","---"}end
vsn1_cli_state=vsn1_cli_state or{r=1,w="",p={t="",b="",v=0,n=0,x=127,d=-1,s=0,i=I(),l=0,h=0,k=0},s={m="",u=0},f={a="",u=0}}
function vsn1_cli_runtime_identity()return"vsn1-cli:2026-06-17-screen-first.2:lcd-init"end
function vsn1_cli_mark_dirty()vsn1_cli_state.r=1 end
local function T(v)return tostring(v or"")end
local function N(v,d)v=tonumber(v)return v and math.floor(v)or d end
function update_param(v,n,x,t,b,s,d,i,c,k)local p=vsn1_cli_state.p p.v=N(v,0)p.n=N(n,0)p.x=N(x,127)p.t=T(t)p.b=T(b)p.s=N(s,0)p.d=N(d,-1)p.k=N(k,0)p.i=I()if type(i)=="table"then for j=1,8 do p.i[j]=T(i[j]or p.i[j])end end if type(c)=="table"then p.l=c[1]and 1 or 0 p.h=c[2]and 1 or 0 else p.l=0 p.h=0 end vsn1_cli_mark_dirty()end
