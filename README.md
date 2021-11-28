# Fuso :  扶桑
A fast, stable, cross-platform and efficient intranet penetration and port forwarding tool

一款 快速 稳定 跨平台 高效的内网穿透，端口转发工具

[![Author](https://img.shields.io/badge/Author-editso-blueviolet)](https://github.com/editso) 
[![Daza](https://img.shields.io/badge/Misc-1x2Bytes-blueviolet)](https://github.com/1x2Bytes) 
[![Daza](https://img.shields.io/badge/Misc-ifishzz-blueviolet)](https://github.com/ifishzz) 
[![Bin](https://img.shields.io/badge/Fuso-Bin-ff69b4)](https://github.com/editso/fuso/releases) 
[![GitHub issues](https://img.shields.io/github/issues/editso/fuso)](https://github.com/editso/fuso/issues) 
[![Github Stars](https://img.shields.io/github/stars/editso/fuso)](https://github.com/editso/fuso) 
[![GitHub forks](https://img.shields.io/github/forks/editso/fuso)](https://github.com/editso/fuso)
[![GitHub license](https://img.shields.io/github/license/editso/fuso)](https://github.com/editso/fuso)
[![Downloads](https://img.shields.io/github/downloads/editso/fuso/total?label=Release%20Download)](https://github.com/editso/fuso/releases/latest)

### Fuso make PortForward & IntranetAccess Easy(😁)

这是一款用于内网穿透 端口转发的神器,帮助运维 开发 快速部署与接入内网 同时支持CobaltStrike 一键转发等功能 

还在因为工具的参数过多而烦恼吗? 我们只实现Socks5 与 端口转发 方便快捷的接入与转发内网流量

### ✨Demo

![Demo](demo/demo.gif)


### 如何使用 (How to use) ❓

1. 你需要先[下载](https://github.com/editso/fuso/releases/latest)或[构建](#Build)Fuso

2. 服务端程序为`fus`, 客户端程序为`fuc`

3. 测试  
   1. 运行`fus`(服务端默认监听`9003`端口) 与 `fuc`(客户端默认转发`80`端口)
   2. 确保服务端端口(`9003`)未被使用
   3. 确保转发的端口(`80`)有服务在运行
   4. 转发成功后需要访问的端口由服务端随机分配
   5. 服务端出现 **Actual access address 0.0.0.0:xxxx**日志则代表转发服务已准备就绪
   
4. 配置  
   `Fuso` 的所有配置都是通过参数传递的方式  
   打开终端运行 `[fus or fuc] --help` 即可获取帮助信息

### Features
| Name                   | <font color="green">✔(Achieved)</font> / <font color="red">❌(Unrealized)</font>) |
| ---------------------- | -------------------------------------------------------------------------------- |
| 基本转发 (Forward)     | <font color="green">✔</font>                                                     |
| 传输加密 (Encrypt)     | ❌                                                                                |
| Sock5代理 (Socks5)     | ❌                                                                                |
| UDP支持  (udp support) | ❌                                                                                |
| 数据传输压缩           | ❌                                                                                |


### 😶部分功能还待完善尽请期待..
