# DIDComm-Ollama Bridge

**IMPORTANT:**
> DIDComm-Ollama crate is provided "as is" without any warranties or guarantees, and by using this framework, users agree to assume all risks associated with its deployment and use including implementing security, and privacy measures in their applications. Affinidi assumes no liability for any issues arising from the use or modification of the project.

## Overview

DIDComm-Ollama Bridge enables a secure and private messaging service that is robust and highly secure, enabling you to chat to your own private personal AI agent from anywhere, from any device.

## Is this truly private?

**Yes.**

The messaging framework is based on DIDComm (insert link) which utilises decentralized identity architecture to provide end-to-end public key cryptography, envelope encryption techniques and freedom to use a number of different crypto algorithms. These are all open-standards so there is no vendor lockin.

Affinidi cannot see your what you are asking or the answers from your AI agents. This is encrypted using your keys on both the AI agent side and client side.

You can choose to run your own DIDComm Mediator/Relay service if want to have complete control over your very own private messaging network.

## Rust Toolchain

This project is being tested against the Rust `2024` edition. You will need to install [Rust Nightly](https://doc.rust-lang.org/book/appendix-07-nightly-rust.html) for this project to work.

## Support & Feedback

If you face any issues or have suggestions, please don't hesitate to contact us using [this link](https://www.affinidi.com/get-in-touch).

### Reporting Technical Issues

If you have a technical issue with the Affinidi Messaging GitHub repo, you can also create an issue directly in GitHub.

If you're unable to find an open issue addressing the problem, [open a new one](https://github.com/affinidi/didcomm-ollama/issues/new). Be sure to include a **title and clear description**, as much relevant information as possible, and a **code sample** or an **executable test case** demonstrating the expected behavior that is not occurring.
