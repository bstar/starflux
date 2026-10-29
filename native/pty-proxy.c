/* STAR/FLUX PTY proxy: libc only, no polling timer or terminal emulator.
 * The persistent owner keeps the original master; tmux owns presentation/history.
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <termios.h>
#include <unistd.h>

static struct termios saved;
static int have_saved, wakefd[2] = {-1,-1};
static volatile sig_atomic_t changed, quitting;
static void restore(void) { if(have_saved) tcsetattr(STDIN_FILENO,TCSANOW,&saved); }
static void wake(int sig) {
    int previous=errno;
    if(sig==SIGWINCH) changed=1; else quitting=1;
    if(wakefd[1]>=0) { char byte=1; ssize_t ignored=write(wakefd[1],&byte,1); (void)ignored; }
    errno=previous;
}
static int all(int fd,const void *data,size_t size) {
    const char *p=data;
    while(size) { ssize_t n=write(fd,p,size); if(n<0&&errno==EINTR)continue; if(n<=0)return -1; p+=n;size-=(size_t)n; }
    return 0;
}
static int geometry(int fd,const char *key) {
    struct winsize size={0};
    if(ioctl(STDIN_FILENO,TIOCGWINSZ,&size)<0)return -1;
    char value[256];
    int n=key ? snprintf(value,sizeof(value),"{\"op\":\"attach\",\"key\":\"%s\",\"rows\":%u,\"cols\":%u}\n",key,size.ws_row,size.ws_col)
        : snprintf(value,sizeof(value),"{\"op\":\"resize\",\"rows\":%u,\"cols\":%u}\n",size.ws_row,size.ws_col);
    if(n<0||(size_t)n>=sizeof(value))return -1;
    return all(fd,value,(size_t)n);
}
static size_t base64(const unsigned char *in,size_t size,char *out) {
    static const char alphabet[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t pos=0;
    for(size_t i=0;i<size;i+=3) {
        unsigned value=(unsigned)in[i]<<16;
        if(i+1<size)value|=(unsigned)in[i+1]<<8;
        if(i+2<size)value|=in[i+2];
        out[pos++]=alphabet[(value>>18)&63];out[pos++]=alphabet[(value>>12)&63];
        out[pos++]=i+1<size?alphabet[(value>>6)&63]:'=';out[pos++]=i+2<size?alphabet[value&63]:'=';
    }
    return pos;
}
int main(int argc,char **argv) {
    if(argc==2&&(!strcmp(argv[1],"--help")||!strcmp(argv[1],"-h"))) {puts("Usage: starflux-pty-proxy SOCKET TERMINAL");return 0;}
    if(argc!=3||strlen(argv[2])>64) {fputs("Usage: starflux-pty-proxy SOCKET TERMINAL\n",stderr);return 2;}
    for(const char *p=argv[2];*p;p++)if(!((*p>='a'&&*p<='z')||(*p>='0'&&*p<='9')||*p=='-'))return 2;
    struct sockaddr_un address={.sun_family=AF_UNIX};
    if(strlen(argv[1])>=sizeof(address.sun_path))return 2;
    strcpy(address.sun_path,argv[1]);
    int sock=socket(AF_UNIX,SOCK_STREAM,0);
    if(sock<0||connect(sock,(struct sockaddr*)&address,sizeof(address))<0) {perror("STAR/FLUX PTY owner");return 1;}
    if(tcgetattr(STDIN_FILENO,&saved)<0) {perror("PTY proxy requires a terminal");return 1;}
    have_saved=1;atexit(restore);
    struct termios raw=saved;
    raw.c_iflag&=~(IGNBRK|BRKINT|PARMRK|ISTRIP|INLCR|IGNCR|ICRNL|IXON);
    raw.c_oflag&=~OPOST;raw.c_lflag&=~(ECHO|ECHONL|ICANON|ISIG|IEXTEN);
    raw.c_cflag&=~(CSIZE|PARENB);raw.c_cflag|=CS8;raw.c_cc[VMIN]=1;raw.c_cc[VTIME]=0;
    if(tcsetattr(STDIN_FILENO,TCSANOW,&raw)<0||pipe(wakefd)<0)return 1;
    for(int i=0;i<2;i++) {fcntl(wakefd[i],F_SETFL,O_NONBLOCK);fcntl(wakefd[i],F_SETFD,FD_CLOEXEC);}
    struct sigaction action={0};action.sa_handler=wake;sigemptyset(&action.sa_mask);
    sigaction(SIGWINCH,&action,NULL);sigaction(SIGTERM,&action,NULL);sigaction(SIGHUP,&action,NULL);
    signal(SIGPIPE,SIG_IGN);
    if(geometry(sock,argv[2])<0)return 1;
    struct pollfd inputs[]={{STDIN_FILENO,POLLIN,0},{sock,POLLIN,0},{wakefd[0],POLLIN,0}};
    unsigned char bytes[65536];char message[87500];
    while(!quitting) {
        if(changed) {changed=0;if(geometry(sock,NULL)<0)return 1;}
        int ready=poll(inputs,3,-1);
        if(ready<0) {if(errno==EINTR)continue;return 1;}
        if(inputs[2].revents&POLLIN)while(read(wakefd[0],bytes,sizeof(bytes))>0) {}
        if(inputs[1].revents&(POLLIN|POLLHUP|POLLERR)) {
            ssize_t n=read(sock,bytes,sizeof(bytes));if(n<0&&errno==EINTR)continue;if(n<=0)break;
            if(all(STDOUT_FILENO,bytes,(size_t)n)<0)break;
        }
        if(inputs[0].revents&(POLLIN|POLLHUP|POLLERR)) {
            ssize_t n=read(STDIN_FILENO,bytes,sizeof(bytes));if(n<0&&errno==EINTR)continue;if(n<=0)break;
            const char *prefix="{\"op\":\"input\",\"data\":\"";size_t len=strlen(prefix);memcpy(message,prefix,len);
            len+=base64(bytes,(size_t)n,message+len);memcpy(message+len,"\"}\n",3);len+=3;
            if(all(sock,message,len)<0)break;
        }
    }
    close(sock);close(wakefd[0]);close(wakefd[1]);return 0;
}
