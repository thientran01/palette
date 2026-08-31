import { Buffer } from "node:buffer";
import crypto from "node:crypto";
import net from "node:net";

function maskFrame(opcode, payload) {
  const mask = crypto.randomBytes(4);
  const masked = Buffer.alloc(payload.length);
  for (let i = 0; i < payload.length; i++) masked[i] = payload[i] ^ mask[i % 4];
  let header;
  if (payload.length < 126) {
    header = Buffer.from([0x80 | opcode, 0x80 | payload.length]);
  } else if (payload.length < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 0x80 | 126;
    header.writeUInt16BE(payload.length, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 0x80 | 127;
    header.writeUInt32BE(0, 2);
    header.writeUInt32BE(payload.length, 6);
  }
  return Buffer.concat([header, mask, masked]);
}

function readFrame(buf) {
  if (buf.length < 2) return null;
  const fin = (buf[0] & 0x80) !== 0;
  const opcode = buf[0] & 0x0f;
  const masked = (buf[1] & 0x80) !== 0;
  let len = buf[1] & 0x7f;
  let off = 2;
  if (len === 126) {
    if (buf.length < 4) return null;
    len = buf.readUInt16BE(2);
    off = 4;
  } else if (len === 127) {
    if (buf.length < 10) return null;
    if (buf.readUInt32BE(2) !== 0) throw new Error("CDP frame larger than 4GB");
    len = buf.readUInt32BE(6);
    off = 10;
  }
  if (masked) off += 4;
  if (buf.length < off + len) return null;
  let payload = buf.subarray(off, off + len);
  if (masked) {
    const mask = buf.subarray(off - 4, off);
    const un = Buffer.alloc(len);
    for (let i = 0; i < len; i++) un[i] = payload[i] ^ mask[i % 4];
    payload = un;
  }
  return { fin, opcode, payload, rest: buf.subarray(off + len) };
}

export function connectCdp(wsUrl) {
  const u = new URL(wsUrl);
  const key = crypto.randomBytes(16).toString("base64");
  return new Promise((resolve, reject) => {
    const sock = net.connect({ host: u.hostname, port: Number(u.port) || 80 });
    let buf = Buffer.alloc(0);
    let open = false;
    let nextId = 1;
    const pending = new Map();
    let parts = [];
    let settled = false;

    const fail = (err) => {
      if (!settled) {
        settled = true;
        reject(err);
      }
    };

    sock.once("error", fail);
    sock.once("connect", () => {
      sock.write(
        `GET ${u.pathname}${u.search} HTTP/1.1\r\n` +
          `Host: ${u.host}\r\n` +
          `Upgrade: websocket\r\n` +
          `Connection: Upgrade\r\n` +
          `Sec-WebSocket-Key: ${key}\r\n` +
          `Sec-WebSocket-Version: 13\r\n\r\n`,
      );
    });

    sock.on("data", (chunk) => {
      buf = Buffer.concat([buf, chunk]);
      if (!open) {
        const end = buf.indexOf("\r\n\r\n");
        if (end === -1) return;
        const head = buf.subarray(0, end).toString("utf8");
        buf = buf.subarray(end + 4);
        if (!head.includes("101")) {
          sock.destroy();
          fail(new Error(`websocket upgrade failed: ${head.split("\r\n")[0]}`));
          return;
        }
        open = true;
        settled = true;
        resolve({
          send(method, params = {}) {
            const id = nextId++;
            sock.write(maskFrame(1, Buffer.from(JSON.stringify({ id, method, params }), "utf8")));
            return new Promise((res, rej) => {
              pending.set(id, { res, rej });
            });
          },
          close() {
            sock.end();
          },
        });
      }
      while (true) {
        let frame;
        try {
          frame = readFrame(buf);
        } catch (err) {
          sock.destroy();
          fail(err);
          return;
        }
        if (!frame) break;
        buf = frame.rest;
        if (frame.opcode === 8) {
          sock.end();
          return;
        }
        if (frame.opcode === 9) {
          sock.write(maskFrame(10, frame.payload));
          continue;
        }
        if (frame.opcode === 1) parts = [frame.payload];
        else if (frame.opcode === 0) parts.push(frame.payload);
        else continue;
        if (!frame.fin) continue;
        const msg = JSON.parse(Buffer.concat(parts).toString("utf8"));
        parts = [];
        const wait = pending.get(msg.id);
        if (!wait) continue;
        pending.delete(msg.id);
        if (msg.error) wait.rej(new Error(msg.error.message || JSON.stringify(msg.error)));
        else wait.res(msg.result);
      }
    });
  });
}
