#define _GNU_SOURCE

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/vm_sockets.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/io.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <unistd.h>

static sigjmp_buf trap_env;
static volatile sig_atomic_t trap_signal;

static int blocked_errno(const char *probe, const char *operation, int err) {
    printf("BLOCKED %s %s errno=%d %s\n", probe, operation, err, strerror(err));
    return 1;
}

static int blocked_note(const char *probe, const char *note) {
    printf("BLOCKED %s %s\n", probe, note);
    return 1;
}

static int breach(const char *probe, const char *note) {
    printf("BREACH %s %s\n", probe, note);
    return 0;
}

static int error_note(const char *probe, const char *note) {
    fprintf(stderr, "ERROR %s %s\n", probe, note);
    return 2;
}

static int drop_to_nobody(const char *probe) {
    if (setgid(65534) != 0) {
        return error_note(probe, "setgid(65534) failed");
    }
    if (setuid(65534) != 0) {
        return error_note(probe, "setuid(65534) failed");
    }
    return 0;
}

static void trap_handler(int signo) {
    trap_signal = signo;
    siglongjmp(trap_env, 1);
}

static int run_with_signal_trap(const char *probe, void (*fn)(void)) {
    struct sigaction action;
    memset(&action, 0, sizeof(action));
    action.sa_handler = trap_handler;
    sigemptyset(&action.sa_mask);

    if (sigaction(SIGILL, &action, NULL) != 0 ||
        sigaction(SIGSEGV, &action, NULL) != 0 ||
        sigaction(SIGBUS, &action, NULL) != 0 ||
        sigaction(SIGTRAP, &action, NULL) != 0 ||
        sigaction(SIGFPE, &action, NULL) != 0) {
        return error_note(probe, "sigaction failed");
    }

    if (sigsetjmp(trap_env, 1) == 0) {
        fn();
        return breach(probe, "privileged instruction returned");
    }

    char note[96];
    snprintf(note, sizeof(note), "signal=%d", trap_signal);
    return blocked_note(probe, note);
}

static int open_nonroot_probe(const char *probe, const char *path) {
    int drop = drop_to_nobody(probe);
    if (drop != 0) {
        return drop;
    }

    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd >= 0) {
        close(fd);
        return breach(probe, "non-root open succeeded");
    }

    int err = errno;
    if (err == EACCES || err == EPERM || err == ENOENT) {
        return blocked_errno(probe, "open", err);
    }
    return error_note(probe, "open failed with unexpected errno");
}

static int probe_dev_mem_nonroot(void) {
    return open_nonroot_probe("dev_mem_nonroot", "/dev/mem");
}

static int probe_proc_kcore_nonroot(void) {
    return open_nonroot_probe("proc_kcore_nonroot", "/proc/kcore");
}

static int probe_virtio_config_write(void) {
    DIR *dir = opendir("/sys/bus/virtio/devices");
    if (dir == NULL) {
        if (errno == ENOENT) {
            return blocked_errno("virtio_config_write", "opendir", errno);
        }
        return error_note("virtio_config_write", "opendir failed");
    }

    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (entry->d_name[0] == '.') {
            continue;
        }

        char path[512];
        int n = snprintf(
            path,
            sizeof(path),
            "/sys/bus/virtio/devices/%s/config",
            entry->d_name
        );
        if (n < 0 || (size_t)n >= sizeof(path)) {
            closedir(dir);
            return error_note("virtio_config_write", "sysfs path too long");
        }

        int fd = open(path, O_WRONLY | O_CLOEXEC);
        if (fd < 0) {
            int err = errno;
            if (err == EACCES || err == EPERM || err == EROFS || err == ENOENT) {
                closedir(dir);
                return blocked_errno("virtio_config_write", path, err);
            }
            continue;
        }

        const void *bad_user_buffer = NULL;
        ssize_t written = syscall(SYS_write, fd, bad_user_buffer, 1);
        int err = errno;
        close(fd);
        closedir(dir);

        if (written >= 0) {
            return breach("virtio_config_write", "write returned success");
        }
        if (err == EFAULT || err == EBADF || err == EINVAL || err == EPERM ||
            err == EACCES || err == EROFS || err == ENODEV) {
            return blocked_errno("virtio_config_write", "write", err);
        }
        return error_note("virtio_config_write", "write failed with unexpected errno");
    }

    closedir(dir);
    return blocked_note("virtio_config_write", "no virtio config file exposed");
}

static void rdmsr_instruction(void) {
    unsigned int lo;
    unsigned int hi;
    unsigned int msr = 0x10;
    __asm__ volatile("rdmsr" : "=a"(lo), "=d"(hi) : "c"(msr));
    (void)lo;
    (void)hi;
}

static void write_cr0_instruction(void) {
    unsigned long value = 0;
    __asm__ volatile("mov %0, %%cr0" : : "r"(value) : "memory");
}

static void write_cr4_instruction(void) {
    unsigned long value = 0;
    __asm__ volatile("mov %0, %%cr4" : : "r"(value) : "memory");
}

static int probe_rdmsr_host_msr(void) {
    return run_with_signal_trap("rdmsr_host_msr", rdmsr_instruction);
}

static int probe_write_cr0(void) {
    return run_with_signal_trap("write_cr0", write_cr0_instruction);
}

static int probe_write_cr4(void) {
    return run_with_signal_trap("write_cr4", write_cr4_instruction);
}

static int probe_ioperm_iopl_nonroot(void) {
    int drop = drop_to_nobody("ioperm_iopl_nonroot");
    if (drop != 0) {
        return drop;
    }

    if (ioperm(0, 1, 1) == 0) {
        return breach("ioperm_iopl_nonroot", "ioperm succeeded");
    }
    if (errno != EPERM) {
        return blocked_errno("ioperm_iopl_nonroot", "ioperm", errno);
    }

    if (iopl(3) == 0) {
        return breach("ioperm_iopl_nonroot", "iopl succeeded");
    }
    if (errno != EPERM) {
        return blocked_errno("ioperm_iopl_nonroot", "iopl", errno);
    }

    return blocked_note("ioperm_iopl_nonroot", "ioperm=EPERM iopl=EPERM");
}

static int probe_vsock_non_allowed_cid(void) {
    int fd = socket(AF_VSOCK, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    if (fd < 0) {
        return blocked_errno("vsock_non_allowed_cid", "socket", errno);
    }

    struct sockaddr_vm addr;
    memset(&addr, 0, sizeof(addr));
    addr.svm_family = AF_VSOCK;
    addr.svm_cid = 99;
    addr.svm_port = 9001;

    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
        close(fd);
        return breach("vsock_non_allowed_cid", "connect succeeded");
    }

    int err = errno;
    if (err != EINPROGRESS) {
        close(fd);
        return blocked_errno("vsock_non_allowed_cid", "connect", err);
    }

    fd_set write_fds;
    FD_ZERO(&write_fds);
    FD_SET(fd, &write_fds);
    struct timeval timeout;
    timeout.tv_sec = 1;
    timeout.tv_usec = 0;

    int ready = select(fd + 1, NULL, &write_fds, NULL, &timeout);
    if (ready == 0) {
        close(fd);
        return blocked_note("vsock_non_allowed_cid", "connect timeout");
    }
    if (ready < 0) {
        err = errno;
        close(fd);
        return blocked_errno("vsock_non_allowed_cid", "select", err);
    }

    int socket_error = 0;
    socklen_t len = sizeof(socket_error);
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &socket_error, &len) != 0) {
        err = errno;
        close(fd);
        return blocked_errno("vsock_non_allowed_cid", "getsockopt", err);
    }
    close(fd);

    if (socket_error == 0) {
        return breach("vsock_non_allowed_cid", "connect completed");
    }
    return blocked_errno("vsock_non_allowed_cid", "connect", socket_error);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <probe>\n", argv[0]);
        return 2;
    }

    if (strcmp(argv[1], "dev_mem_nonroot") == 0) {
        return probe_dev_mem_nonroot();
    }
    if (strcmp(argv[1], "virtio_config_write") == 0) {
        return probe_virtio_config_write();
    }
    if (strcmp(argv[1], "rdmsr_host_msr") == 0) {
        return probe_rdmsr_host_msr();
    }
    if (strcmp(argv[1], "write_cr0") == 0) {
        return probe_write_cr0();
    }
    if (strcmp(argv[1], "write_cr4") == 0) {
        return probe_write_cr4();
    }
    if (strcmp(argv[1], "proc_kcore_nonroot") == 0) {
        return probe_proc_kcore_nonroot();
    }
    if (strcmp(argv[1], "ioperm_iopl_nonroot") == 0) {
        return probe_ioperm_iopl_nonroot();
    }
    if (strcmp(argv[1], "vsock_non_allowed_cid") == 0) {
        return probe_vsock_non_allowed_cid();
    }

    fprintf(stderr, "unknown probe: %s\n", argv[1]);
    return 2;
}
