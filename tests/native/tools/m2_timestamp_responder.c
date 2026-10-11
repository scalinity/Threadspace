/* Disposable qualification-only HTTP Unix-socket timestamp responder. */
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/stat.h>
#include <sys/sysctl.h>
#include <time.h>
#include <poll.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>

typedef struct { char runtime[65], epoch[65], challenge[65]; } Planned;
static Planned plan[56];
static volatile sig_atomic_t stopping;
static void stop(int s) { (void)s; stopping=1; }
static int token(const char *s) {
    size_t n=strlen(s); if (!n || n>64) return 0;
    for (size_t i=0;i<n;i++) if (!((s[i]>='a'&&s[i]<='z')||(s[i]>='0'&&s[i]<='9')||s[i]=='-')) return 0;
    return 1;
}
static void durable(FILE *f) { if (fflush(f)||fsync(fileno(f))) exit(74); }
static unsigned long long stamp(void) {
    struct timespec t; if (clock_gettime(CLOCK_UPTIME_RAW,&t)) exit(74);
    return (unsigned long long)t.tv_sec*1000000000ULL+(unsigned long long)t.tv_nsec;
}
static int send_all(int fd,const char *s,size_t n) {
    while(n) { ssize_t k=send(fd,s,n,0); if(k<=0) return 0; s+=k; n-=(size_t)k; } return 1;
}
int main(int argc,char **argv) {
    if(argc!=7 && argc!=9) return 64;
    const char *dir=argv[1], *run=argv[2], *boot=argv[3], *inc=argv[4], *manifest=argv[5];
    int delay_seq=argc==9?atoi(argv[7]):-1, delay_ms=argc==9?atoi(argv[8]):0;
    if(!token(run)||!token(boot)||!token(inc)||!token(manifest)||delay_ms<0||delay_ms>100) return 64;
    struct stat ds; if(lstat(dir,&ds)||!S_ISDIR(ds.st_mode)||ds.st_uid!=getuid()||(ds.st_mode&0777)!=0700) return 77;
    char actual_boot[65]; size_t bn=sizeof actual_boot;
    if(sysctlbyname("kern.bootsessionuuid",actual_boot,&bn,NULL,0)) return 74;
    for(size_t i=0;actual_boot[i];i++) if(actual_boot[i]>='A'&&actual_boot[i]<='Z') actual_boot[i]+=32;
    if(strcmp(actual_boot,boot)) return 77;
    FILE *pf=fopen(argv[6],"r"); if(!pf) return 66;
    for(int i=0;i<56;i++) {
        int seq; if(fscanf(pf,"%d %64s %64s %64s",&seq,plan[i].runtime,plan[i].epoch,plan[i].challenge)!=4||seq!=i||!token(plan[i].runtime)||!token(plan[i].epoch)||!token(plan[i].challenge)) return 65;
        for(int j=0;j<i;j++) if(!strcmp(plan[j].challenge,plan[i].challenge)) return 65;
    }
    char extra[2]; if(fscanf(pf,"%1s",extra)==1) return 65; fclose(pf);
    char socket_path[104], log_path[1024];
    if(snprintf(socket_path,sizeof socket_path,"%s/s.sock",dir)>=(int)sizeof socket_path) return 64;
    snprintf(log_path,sizeof log_path,"%s/native.jsonl",dir);
    int logfd=open(log_path,O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW,0600); if(logfd<0) return 73;
    FILE *log=fdopen(logfd,"w"); if(!log) return 74;
    fprintf(log,"{\"event\":\"start\",\"runId\":\"%s\",\"bootId\":\"%s\",\"responderId\":\"%s\",\"manifestSha256\":\"%s\",\"pid\":%d,\"uid\":%d,\"clock\":\"CLOCK_UPTIME_RAW\"}\n",run,boot,inc,manifest,getpid(),getuid()); durable(log);
    int server=socket(AF_UNIX,SOCK_STREAM,0); if(server<0) return 71;
    struct sockaddr_un addr={0}; addr.sun_family=AF_UNIX; strlcpy(addr.sun_path,socket_path,sizeof addr.sun_path);
    umask(077); if(bind(server,(struct sockaddr*)&addr,sizeof addr)||chmod(socket_path,0600)||listen(server,4)) return 71;
    signal(SIGTERM,stop); signal(SIGINT,stop); signal(SIGPIPE,SIG_IGN);
    unsigned expected=0, attempts=0, answered=0;
    while(!stopping && attempts<128) {
        struct pollfd p={server,POLLIN,0}; if(poll(&p,1,100)<=0) continue;
        int fd=accept(server,NULL,NULL); if(fd<0) continue; attempts++;
        uid_t uid; gid_t gid; int same=getpeereid(fd,&uid,&gid)==0 && uid==getuid();
        struct timeval timeout={2,0}; setsockopt(fd,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof timeout); setsockopt(fd,SOL_SOCKET,SO_SNDTIMEO,&timeout,sizeof timeout);
        char request[4097]={0}; size_t used=0;
        while(used<4096 && !strstr(request,"\r\n\r\n")) { ssize_t n=recv(fd,request+used,4096-used,0); if(n<=0) break; used+=(size_t)n; request[used]=0; }
        char rr[65],bb[65],ii[65],rt[65],ep[65],cc[65],version[16],tail[2]; unsigned seq=9999;
        int valid=same && strstr(request,"\r\n\r\n") && sscanf(request,"GET /%64[^/]/%64[^/]/%64[^/]/%64[^/]/%64[^/]/%u/%64s %15s%1[^\r\n]",rr,bb,ii,rt,ep,&seq,cc,version,tail)==8;
        const char *reason="malformed";
        if(valid) {
            valid=token(rr)&&token(bb)&&token(ii)&&token(rt)&&token(ep)&&token(cc)&&!strcmp(version,"HTTP/1.1");
            if(valid && strcmp(rr,run)) { valid=0; reason="wrong-run"; }
            else if(valid && strcmp(bb,boot)) { valid=0; reason="wrong-boot"; }
            else if(valid && strcmp(ii,inc)) { valid=0; reason="wrong-responder"; }
            else if(valid && (seq!=expected||seq>=56)) { valid=0; reason="sequence"; }
            else if(valid && (strcmp(rt,plan[seq].runtime)||strcmp(ep,plan[seq].epoch)||strcmp(cc,plan[seq].challenge))) { valid=0; reason="challenge-runtime-epoch"; }
        }
        fprintf(log,"{\"event\":\"received\",\"attempt\":%u,\"sequence\":%u,\"accepted\":%s,\"reason\":\"%s\"}\n",attempts,seq,valid?"true":"false",valid?"accepted":reason); durable(log);
        if(!valid) {
            const char *bad="HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
            int ok=send_all(fd,bad,strlen(bad));
            fprintf(log,"{\"event\":\"rejected\",\"attempt\":%u,\"answered\":%s}\n",attempts,ok?"true":"false"); durable(log); close(fd); continue;
        }
        unsigned long long ns=stamp();
        char body[1024];
        snprintf(body,sizeof body,"{\"sequence\":%u,\"runId\":\"%s\",\"bootId\":\"%s\",\"responderId\":\"%s\",\"runtimeId\":\"%s\",\"sourceEpoch\":\"%s\",\"challenge\":\"%s\",\"manifestSha256\":\"%s\",\"nativeNs\":\"%llu\",\"clock\":\"CLOCK_UPTIME_RAW\"}",seq,run,boot,inc,rt,ep,cc,manifest,ns);
        fprintf(log,"{\"event\":\"generated\",\"attempt\":%u,\"response\":%s}\n",attempts,body); durable(log);
        if((int)seq==delay_seq) { struct timespec d={0,delay_ms*1000000L}; nanosleep(&d,NULL); }
        char header[160]; snprintf(header,sizeof header,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n",strlen(body));
        int ok=send_all(fd,header,strlen(header))&&send_all(fd,body,strlen(body));
        fprintf(log,"{\"event\":\"%s\",\"attempt\":%u,\"sequence\":%u}\n",ok?"answered":"incomplete",attempts,seq); durable(log);
        expected++; if(ok) answered++; close(fd);
    }
    fprintf(log,"{\"event\":\"stop\",\"attempts\":%u,\"generated\":%u,\"answered\":%u}\n",attempts,expected,answered); durable(log);
    close(server); unlink(socket_path); fclose(log); return 0;
}
