# Test servers for remote panels

Podman containers on 127.0.0.1 (user `tester`, password `testpass`).

| Server | Port | Container |
|---|---|---|
| SFTP (`/upload` writable) | 2222 | `on-test-sftp` (docker.io/atmoz/sftp:alpine) |
| FTP | 2121, passive 21000-21010 | `on-test-ftp` (docker.io/delfer/alpine-ftp-server) |
| FTPS explicit, self-signed | 2990, passive 21100-21110 | `on-test-ftps` (same image, TLS) |

Start (from Git Bash; `$S` = a folder holding `ftps.crt` / `ftps.key`, made with
`openssl req -x509 -newkey rsa:2048 -nodes -keyout ftps.key -out ftps.crt -days 30 -subj "/CN=localhost"`):

```sh
podman run -d --name on-test-sftp -p 127.0.0.1:2222:22 docker.io/atmoz/sftp:alpine tester:testpass:1001:1001:upload
podman run -d --name on-test-ftp -p 127.0.0.1:2121:21 -p 127.0.0.1:21000-21010:21000-21010 \
  -e USERS="tester|testpass" -e ADDRESS=127.0.0.1 docker.io/delfer/alpine-ftp-server
MSYS_NO_PATHCONV=1 podman run -d --name on-test-ftps -p 127.0.0.1:2990:21 -p 127.0.0.1:21100-21110:21100-21110 \
  -e USERS="tester|testpass" -e ADDRESS=127.0.0.1 -e MIN_PORT=21100 -e MAX_PORT=21110 \
  -e TLS_CERT=/certs/ftps.crt -e TLS_KEY=/certs/ftps.key -v "$(cygpath -w "$S"):/certs:ro" docker.io/delfer/alpine-ftp-server
# Windows curl (Schannel) needs this, else FTPS fails with 522:
podman exec on-test-ftps sh -c 'echo "require_ssl_reuse=NO" >> /etc/vsftpd/vsftpd.conf' && podman restart on-test-ftps
```

Live tests read: `ON_TEST_SFTP=127.0.0.1:2222`, `ON_TEST_FTP=127.0.0.1:2121`, `ON_TEST_FTPS=127.0.0.1:2990`,
`ON_TEST_USER=tester`, `ON_TEST_PASSWORD=testpass`, and run with `cargo test --release -- --ignored`.

Stop and remove: `podman rm -f on-test-sftp on-test-ftp on-test-ftps`
