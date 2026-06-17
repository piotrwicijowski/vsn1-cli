local function I()return{"---","---","---","---","---","---","---","---"}end
z=vsn1_cli_state or{r=1,w="",p={t="",b="",v=0,n=0,x=127,d=-1,s=0,i=I(),l=0,h=0,k=0},s={m="",u=0},f={a="",u=0}}vsn1_cli_state=z
c=c or{{0,0,0},{255,255,255},{64,160,255}}
function P(v,n,x,t,b,s,d,i,c,k)local p=z.p if v~=nil then p.v=v end if n~=nil then p.n=n end if x~=nil then p.x=x end if t~=nil then p.t=t end if b~=nil then p.b=b end if s~=nil then p.s=s end if d~=nil then p.d=d end if i then for j=1,8 do p.i[j]=i[j]or p.i[j] end end if c then if c[1]~=nil then p.l=c[1]and 1 or 0 end if c[2]~=nil then p.h=c[2]and 1 or 0 end end if k~=nil then p.k=k end z.r=1 end
update_param=P
function S(m)z.s.m=m z.r=1 end
function F(a)z.f.a=a z.r=1 end
function A(t)if t==5 then z.s.u=os.clock()+5 else z.f.u=os.clock()+1 end z.r=1 end
