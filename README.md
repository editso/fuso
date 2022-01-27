# Fuso :  扶桑
A fast, stable, cross-platform and efficient intranet penetration and port forwarding tool

一款 快速🚀 稳定 跨平台 高效的内网穿透，端口转发工具

[![Author](https://img.shields.io/badge/Author-editso-blueviolet)](https://github.com/editso) 
[![Daza](https://img.shields.io/badge/Misc-1x2Bytes-blueviolet)](https://github.com/B1eed) 
[![Daza](https://img.shields.io/badge/Misc-ifishzz-blueviolet)](https://github.com/ifishzz) 
[![Bin](https://img.shields.io/badge/Fuso-Bin-ff69b4)](https://github.com/editso/fuso/releases) 
[![GitHub issues](https://img.shields.io/github/issues/editso/fuso)](https://github.com/editso/fuso/issues) 
[![Github Stars](https://img.shields.io/github/stars/editso/fuso)](https://github.com/editso/fuso) 
[![GitHub forks](https://img.shields.io/github/forks/editso/fuso)](https://github.com/editso/fuso)
[![GitHub license](https://img.shields.io/github/license/editso/fuso)](https://github.com/editso/fuso)
[![Downloads](https://img.shields.io/github/downloads/editso/fuso/total?label=Release%20Download)](https://github.com/editso/fuso/releases/latest)

### Fuso make PortForward & IntranetAccess Easy

👉 这是一款用于内网穿透 端口转发的工具,帮助运维,开发人员快速部署与接入内网

👉 传输采用`Rsa + Aes`加密

👉 该项目可直接当做库来使用

👉 目前该项目还处于初步开发阶段，欢迎提出功能与意见´◡`

👉  项目将在年后进行异步库迁移 smol => tokio 可自定义运行时, 提高性能与扩展性

### ✨Demo

![Demo](demo/demo.gif)


### 👀如何使用 (How to use) ❓
1. 你需要先[下载](https://github.com/editso/fuso/releases/latest)或[构建](#Build)`Fuso`
2. `fuso` 分为客户端(`fuc`)与服务端(`fus`)
3. 将你下载或构建好的`fus`程序[部署](#服务端部署)到服务器
4. 将你下载或构建好的`fuc`程序[部署](#客户端部署)到你需要穿透的电脑上

#### 服务端部署
1. 采用参数传递的形式来部署无需任何配置文件, 并且配置简单大多情况下可使用默认配置

2. **参数说明**    
`-h`: 绑定的地址
`-p`: 监听的端口, 也就是客户端需要连接到服务端的端口  
`-l`: 日志信息级别 (`debug`, `info`, `trace`, `error`, `warn`)  
`--auth`: 认证方式 (预留, 暂未实现)   
`--secret`: 密码 (预留, 暂未实现)  
`-v`: 该参数打印的版本目前无效  
`-h`: 获取帮助信息


#### 客户端部署
1. 客户端配置相对服务端来说可能会复杂一点, 但大多数情况下也可使用默认配置
 
2. **参数说明**   
fuc [options] <server-host> <server-port>  
`<server-host>`: 服务端地址, 支持域名  
`<server-port>`: 服务端监听的端口  
`-h` | `--forward-host`: 需要转发的地址, 也就是穿透地址  
`-p` | `--forward-host`: 转发的端口, 需要配合 `-h`参数  
`-b` | `--visit-port`: 真实映射成功后访问的端口号, 不指定将自动分配  
`-n` | `--name`: 一个标识, 映射服务的名称   
`-t` | `--forward-type`: 转发类型, 默认自动判定类型 支持: [`socks5`, `forward`]  
`--crypt-type`: 传输加密类型 默认使用`aes`加密  
`--crypt-secret`: 传输加密密钥, 默认随机  
`--handsnake`: 前置握手方式, 默认不进行前置握手, 支持: [`websocket`]  
`--bridge-host`: 本地桥接绑定地址    
`--bridge-port`: 本地桥接监听端口    
`--s5-pwd`: `Socks5`认证时的连接密码, 默认不需要  
`-P | --fuso-pwd`: 连接到服务端所需密码(预留, 暂未实现)  
`-l`: 日志信息级别 (`debug`, `info`, `trace`, `error`, `warn`)  

```
# 一个转发例子
# 服务端绑定在 xxx.xxx.xxx.xxx:9003
# 转发内网中 10.10.10.8:80 到 xxx.xxx.xxx.xxx:8080
# 需要注意的是:
# 10.10.10.8 必须是能 ping 通的
# 80 端口必须有服务在运行
# 服务端已经在运行,并且服务端80端口没有被占用

# 运行: 
> fuc -h 10.10.10.8 -p 80 -b 8080 xxx.xxx.xxx.xxx 9003

# 该命令运行后既可以是转发模式, 也可以是Socks5模式都可以使用8080端口进行访问


# 一个桥接例子
# 什么时候能用到桥接模式呢? 
# 比如: 
#  你的内网中只有一台机器可以出网, 但是我想访问不能出网机器上所运行的服务
#  那么此时就可以使用桥接模式, 通过可以在出网的机器上开启桥接模式来转发不能出网的服务
#  前提是你不能出网的机器和可以出网的机器在同一个内网中, 并且可以相互 ping 通

# 在可以出网的机器上开启桥接 (0.0.0.0:9004)
# 假设可以出网的内网ip地址为 10.10.10.5
# 运行:
> fuc -h 10.10.10.8 -p 80 -b 8080 --bridge-host 0.0.0.0 --bridge-port 9004 xxx.xxx.xxx.xxx 9003

# 在不可以出网的机器上需要穿透80服务, 并且服务端监听8081端口
# 此时fuc的服务端地址就不应该是服务器地址, 因为并不能出网, 所以需要连接到开启桥接服务的地址
# 运行:
> fuc -h 127.0.0.1 -p 80 -b 8081 10.10.10.5 9004


# 另一种桥接做法
# 假设不能出网机器的ip地址为 10.10.10.6
# 在可以出网的机器上运行:
> fuc -h 10.10.10.6 -p 80 -b 8081 xxx.xxx.xxx.xxx 9003

# 此时也可以达到效果, 但是这样一来可以出网的就无法转发自己所监听的服务

```


### 🤔Features
| Name           | <font color="green">✔(Achieved)</font> / <font color="red">❌(Unrealized)</font>) |
| -------------- | -------------------------------------------------------------------------------- |
| 基本转发       | <font color="green">✔</font>                                                     |
| 传输加密       | <font color="green">✔</font>                                                     |
| Socks5代理     | <font color="green">✔</font>                                                     |
| Socks5 Udp转发 | <font color="green">✔</font>                                                     |
| Udp (kcp)支持  | ❌                                                                                |
| 多映射         | <font color="green">✔</font>                                                     |
| 级联代理       | <font color="green">✔</font>                                                     |
| 数据传输压缩   | ❌                                                                                |
| Websocket      | <font color="green">✔</font>                                                     |
| `Rsa`加密      | <font color="green">✔</font>                                                     |
| `Aes`加密      | <font color="green">✔</font>                                                     |

### 😶部分功能还待完善敬请期待..

### 注意
- 本项目所用技术**仅用于学习交流**，**请勿直接用于任何商业场合和非法用途**。

