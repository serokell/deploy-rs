# Client-Server Split

## What is this?

This is an RFC, for a major change in design. I propose to split `deploy-rs` into two fully self standing executables, a
server and a client. This would allow for additional flexibility and some issues are impossible to solve nicely with the
current design.

## The design

I propose to split `deploy-rs` into a server and a client, `deploy` and `activate` respectively. Basically, while now
`activate` is just a helper, this RFC proposes to make it a self standing program. The basic jist is that the two
components would communicate over a open ssh connection, using plain REST (this is important later). That would allow us
bidirectional flow of data between the two components which greatly increases the things that can be implemented.

## Benefits

Fixes of sudo and ssh password login, more possibilities like better error reporting. A daemon mode and limiting the
number of ssh connections. Parallelism and plugins/secrets (maybe).

### Daemon

The server component could be made in such a way that it running as a daemon listening over ssh would be equal
to running in daemon mode and listening on TCP/IP. We'd just enable authentication and change over what it
communicates. We could even call it `single-client-mode` and `multi-client-mode`.

## Drawbacks

```
A simple multi-profile Nix-flake deploy tool.
```
could be considered false advertising and general increased complexity.

## Bonus

Since the `server` component will get scraped anyway why not write it in Haskell? If we stick to a REST API compatibility won't be an issue and contributors shouldn't be an issue either. There are more than enough Haskellers in the Nix community. We also all know how amazing Haskell is. Rust's compilation times are not that much better when compared to Haskell either.

## Related
### Issues
- [x] - [Build on target server? #12](https://github.com/serokell/deploy-rs/issues/12)  
  By splitting deploy-rs into a server and client component, building on the target would become easier.
- [x] - [Using different nixpkgs channels for different machines causes excessive builds #19](https://github.com/serokell/deploy-rs/issues/19)  
  The fix proposed is to break the API, why not when we're already redesigning the thing.
- [x] - [Secret management support #20](https://github.com/serokell/deploy-rs/issues/20)  
  If the server component became self standing and could act on it's own, it could conceivably replace `vault-agent`
  and other secret management agents.
- [ ] - [Weird text spacing with the new logger #44](https://github.com/serokell/deploy-rs/issues/44)  
- [x] - [Parallel Deploys #46](https://github.com/serokell/deploy-rs/issues/46)  
  If the client merely told the server to activate a profile, then the "telling" happen in parallel while the
  activation could still be left serial and no special DAG would be needed.
- [x] - [Try to limit the number of SSH connections #54](https://github.com/serokell/deploy-rs/issues/54)  
  By restricting ourselves to one SSH connection to one per node, which is easy to do with a server client design,
  we'd solve this too.
- [ ] - [How to trouble-shoot no confirmation signal coming back #55](https://github.com/serokell/deploy-rs/issues/55)  
  Maybe? Not enough information.
- [ ] - [deploy stuck when target network goes down during deploy #57](https://github.com/serokell/deploy-rs/issues/57)  
  Maybe? Again not enough information.
- [ ] - [Rollback not working as expected. #68](https://github.com/serokell/deploy-rs/issues/68)  
  Maybe? Again not enough information.
- [ ] - [Specify MSRV #69](https://github.com/serokell/deploy-rs/issues/69)  
- [ ] - [semver #72](https://github.com/serokell/deploy-rs/issues/72)  
- [x] - [deploy install #76](https://github.com/serokell/deploy-rs/issues/76)  
  Just needs implementing on the server side, but not a direct fix.
- [x] - [deployd-rs #77](https://github.com/serokell/deploy-rs/issues/77)  
  Basically what this proposal is about, if the server component could act independently of the client and accept
  client connections not just over stdin/stdout but over TCP too, then it could be ran as a daemon easily.
- [x] - [Password based sudo #78](https://github.com/serokell/deploy-rs/issues/78)  
  If the client used `SUDO_ASKPASS` to have sudo ask for the password there, that could be hooked into the server
  component. Then it could await a password from a client with a certain timeout. Basically this requires a
  bidirectional connection between the client and server.
- [ ] - [Check generated fstab for changes #83](https://github.com/serokell/deploy-rs/issues/83)  
- [x] - [example system doesn't work. #85](https://github.com/serokell/deploy-rs/issues/85)  
  This would be a side effect since deploy-rs would change so much examples would have to be reworked and expanded.
- [ ] - [Magic rollback is not working if previous version was not deployed with deploy-rs #86](https://github.com/serokell/deploy-rs/issues/86)  
- [x] - [deploy-rs takes a long time to evaluate machines #89](https://github.com/serokell/deploy-rs/issues/89)  
  Again this may end up being a side effect of a larger rework.
- [x] - [Is there an obvious way to ignore a specific activation failure (or not activate before next boot?) #91](https://github.com/serokell/deploy-rs/issues/91)  
  Side effect but not a direct fix.

### Pull Requests
- [ ] - [Skip checks by default #102](https://github.com/serokell/deploy-rs/pull/102)  
- [x] - [Rename deploy-rs to yeet #110](https://github.com/serokell/deploy-rs/pull/110)  
  With such major changes a rebrand would be OK, even beneficial.
- [ ] - [optimize release build for size #111](https://github.com/serokell/deploy-rs/pull/111)  
- [ ] - [add cmd completion #113](https://github.com/serokell/deploy-rs/pull/113)  
- [ ] - [add store root for nixos install #114](https://github.com/serokell/deploy-rs/pull/114)  
  No idea what this is supposed to be or do.
- [x] - [Refactor data views into actions #115](https://github.com/serokell/deploy-rs/pull/115)  
  This seems like general code improvements, should be merged before a major rewrite, if we decide to keep this in
  Rust, more on that later.
- [x] - [Expand environmental variables in sshOpts #116](https://github.com/serokell/deploy-rs/pull/116)  
  We should remember this when we design the protocol.
- [x] - [Refactor data to settings (specificity) I/V #117](https://github.com/serokell/deploy-rs/pull/117)  
  This one confuses me too, but may be worth merging before carrying out what this RFC proposes.
- [x] - [ref data into data II/V #118](https://github.com/serokell/deploy-rs/pull/118)  
- [x] - [ref delploy data aka target parser III/V #119](https://github.com/serokell/deploy-rs/pull/119)  
- [x] - [ref flake instrumentation into adapter IV/V #120](https://github.com/serokell/deploy-rs/pull/120)  
- [x] - [ref homologate data structures V/V #121](https://github.com/serokell/deploy-rs/pull/121)  
- [x] - [iog master diff #126](https://github.com/serokell/deploy-rs/pull/126)  
  This one should honesty be broken up into digestible chunks we could have a discussion about and merge one by one, again, before this RFC.
- [ ] - [Automatically update flake.lock to the latest version #134](https://github.com/serokell/deploy-rs/pull/134)  
  good bot
- [x] - [chore: cleanup root and examples #135](https://github.com/serokell/deploy-rs/pull/135)  
  definitely should be merged before work on this RFC starts.
	  
### Summary

Here I'll separate issues and PRs into several logical groups according to their relation to this RFC.

### Should be merged (if we're not rewriting this from scratch)
- [Refactor data to settings (specificity) I/V #117](https://github.com/serokell/deploy-rs/pull/117)  
- [ref data into data II/V #118](https://github.com/serokell/deploy-rs/pull/118)  
- [ref delploy data aka target parser III/V #119](https://github.com/serokell/deploy-rs/pull/119)  
- [ref flake instrumentation into adapter IV/V #120](https://github.com/serokell/deploy-rs/pull/120)  
- [ref homologate data structures V/V #121](https://github.com/serokell/deploy-rs/pull/121)  
- [iog master diff #126](https://github.com/serokell/deploy-rs/pull/126)  
- [chore: cleanup root and examples #135](https://github.com/serokell/deploy-rs/pull/135)  

### Should be fixed/Implemented in the MVP
- [Expand environmental variables in sshOpts #116](https://github.com/serokell/deploy-rs/pull/116)  
- [Build on target server? #12](https://github.com/serokell/deploy-rs/issues/12)  
- [Parallel Deploys #46](https://github.com/serokell/deploy-rs/issues/46)  
- [deployd-rs #77](https://github.com/serokell/deploy-rs/issues/77)  

### Directly related and a major motivation
- [Try to limit the number of SSH connections #54](https://github.com/serokell/deploy-rs/issues/54)  
- [Password based sudo #78](https://github.com/serokell/deploy-rs/issues/78)  
- [Parallel Deploys #46](https://github.com/serokell/deploy-rs/issues/46)  

### Nice side effects
- [Build on target server? #12](https://github.com/serokell/deploy-rs/issues/12)  
- [Using different nixpkgs channels for different machines causes excessive builds #19](https://github.com/serokell/deploy-rs/issues/19)  
- [example system doesn't work. #85](https://github.com/serokell/deploy-rs/issues/85)  
- [deploy-rs takes a long time to evaluate machines #89](https://github.com/serokell/deploy-rs/issues/89)  
- [Is there an obvious way to ignore a specific activation failure (or not activate before next boot?) #91](https://github.com/serokell/deploy-rs/issues/91)  
