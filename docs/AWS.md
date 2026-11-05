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

## With CloudFront and your own domain

[`deploy/aws/bantu-cloudfront.yaml`](../deploy/aws/bantu-cloudfront.yaml) gives you a normal
HTTPS address such as `https://bantu.example.com`, with no VPN app on your devices. Idle
auto-stop, the wake link and the shell work the same way.

You need a domain in a Route 53 hosted zone and an ACM certificate **in us-east-1** covering
the app and preview names, such as `*.bantu.example.com` and `bantu.example.com`. Then:

```sh
aws ec2 describe-managed-prefix-lists \
  --filters Name=prefix-list-name,Values=com.amazonaws.global.cloudfront.origin-facing \
  --query "PrefixLists[0].PrefixListId" --output text --region <your region>

aws cloudformation deploy \
  --stack-name bantu \
  --template-file deploy/aws/bantu-cloudfront.yaml \
  --capabilities CAPABILITY_IAM \
  --parameter-overrides \
    HostedZoneId=Z123… CloudFrontPrefixListId=pl-… CertificateArn=arn:aws:acm:us-east-1:… \
    AppDomain=bantu.example.com PreviewDomain=preview.bantu.example.com \
    OriginDomain=origin.bantu.example.com PreviewOriginDomain=preview-origin.bantu.example.com
```

CloudFront takes about 15 minutes to deploy, and the machine needs about 5 more to set up.

How it's protected:

- **HTTPS the whole way.** CloudFront connects to the machine over HTTPS, with a Let's Encrypt
  certificate that Caddy on the machine obtains and renews.
- **Only your CloudFront reaches the daemon.** Port 443 only admits CloudFront's address
  range. Every CloudFront customer shares that range, so Caddy also requires a secret header
  that only your distributions send.
- **Port 80 is open** to the internet, because Let's Encrypt's renewal check uses it. It serves
  only that check and a redirect to HTTPS.
- **Previews get their own distribution**, so previewed pages stay on a different origin from
  your daemon's token.
- **Every request still needs the daemon's token**, except previews, which use their own
  grants. Unlike the Tailscale version, this address can be reached from the whole internet,
  so anyone holding your token can run agents as you.

It costs about $4.50 a month more than Tailscale: ≈ $3.65 for the Elastic IP, which is billed
while stopped too, and $0.50 for the hosted zone. CloudFront's free tier (1 TB out, 10 million
requests) covers normal use. While the machine is stopped, the address shows a CloudFront
error page; open the wake link first.

## Launch Stack button

The AWS console can open either template from a link, which saves using the CLI:

```
https://console.aws.amazon.com/cloudformation/home#/stacks/create/review?templateURL=https://<bucket>.s3.amazonaws.com/bantu-tailscale.yaml&stackName=bantu
```

CloudFormation only accepts templates from S3, so this needs a public copy. See
[RELEASING.md](RELEASING.md) to publish one with each release.

## Why Tailscale by default?

Tailscale is end-to-end encrypted, opens no ports, needs no domain, and is free for personal use.
The CloudFront version needs a domain and a certificate, and opens the daemon to the internet
behind its token. CloudFront's private "VPC origins" would avoid that, but they need a NAT gateway
(≈ $33/month) for the machine to reach the internet.

> Status: both templates pass `cfn-lint` and their boot scripts are syntax-checked, and the
> CloudFront version's Caddy routing was tested with the real binary. Neither has been deployed
> end to end yet. Please open an issue if anything goes wrong.
