# Bantu on your own AWS account

[`deploy/aws/bantu-tailscale.yaml`](../deploy/aws/bantu-tailscale.yaml) creates a small
Graviton machine that runs `bantud` around the clock when you need it, and stops itself
when you don't. You pay AWS directly, at cost. Bantu has no servers in the path.

What you get:

- **No open ports.** The machine is reachable only over your Tailscale network, with HTTPS
  at `https://<name>.<tailnet>.ts.net`. Your phone and laptops on the tailnet can use it.
- **Idle auto-stop.** After `IdleStopMinutes` (default 60) with no agent working, no
  terminal or browser connected, and nobody logged in, the machine stops itself.
- **Wake link.** A private URL (a free Lambda function URL) starts it again from any
  browser. It's usually ready about a minute later.
- **Shell without SSH.** AWS Session Manager, for `claude login` and `gh auth login`.

## Cost (us-east-1, on-demand)

| | Monthly |
|---|---|
| t4g.medium, used 4 h/day (≈120 h) | ≈ $4 |
| 30 GiB gp3 disk (billed while stopped too) | ≈ $2.40 |
| Public IPv4 while running | ≈ $0.60 |
| Lambda wake link, Tailscale (personal) | $0 |
| **Total** | **≈ $7** |

Always-on t4g.medium is about $30/month. Use `t4g.small` for one or two agents.

## Deploy

You need the AWS CLI signed in to your account and a
[Tailscale auth key](https://login.tailscale.com/admin/settings/keys). Make the key
reusable if you may recreate the stack, and consider an expiry. It's only used on first
boot.

```sh
aws cloudformation deploy \
  --stack-name bantu \
  --template-file deploy/aws/bantu-tailscale.yaml \
  --capabilities CAPABILITY_IAM \
  --parameter-overrides TailscaleAuthKey=tskey-auth-… MachineName=bantu

aws cloudformation describe-stacks --stack-name bantu \
  --query "Stacks[0].Outputs" --output table
```

Or upload the template in the CloudFormation console (Create stack → Upload a template
file).

## First run

Setup takes about 3 minutes after the stack finishes (`/var/log/bantu-setup.log` on the
machine).

```sh
aws ssm start-session --target <InstanceId>   # from the stack outputs
sudo -iu ec2-user
claude                                        # sign in once
gh auth login                                 # for PRs and checks
bantud url --qr                               # scan with your phone
```

Then open `https://<name>.<tailnet>.ts.net/#token=…` and add a repository by URL (for
example `owner/repo`); Bantu clones it into `~/bantu/repos`.

Bookmark the **WakeLink** output. When the machine has idled out, open it, wait a minute,
then open Bantu.

## Remove

```sh
aws cloudformation delete-stack --stack-name bantu
```

This deletes the machine and its disk, including any work not pushed to a remote.

## Why Tailscale and not CloudFront?

CloudFront can't reach a plain EC2 instance over HTTPS without a domain and certificate
on the instance, and its private "VPC origins" need a NAT gateway (≈ $33/month) for the
machine to reach the internet. Both break "cheap and simple", and plain HTTP between
CloudFront and the machine would expose your token. Tailscale is end-to-end encrypted,
opens no ports, and is free for personal use. A CloudFront variant (with your own domain)
is on the roadmap.

> Status: the template passes `cfn-lint` and its boot script is syntax-checked, but it
> hasn't been deployed end to end yet. Please open an issue if anything goes wrong.
