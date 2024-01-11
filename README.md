# S&F Scrapbook Helper
A simple tool to help you fill the scrapbook. It searches the entire Hall of Fame with up to 10 newly created background characters and displays the characters with the most not yet collected items. 
You can then either attack the players manually (which might cost a mushroom), or click the automate button to battle the best character as soon, as it is free. 
Both normal and SSO (S&F Account) characters are supported.
If you have multiple accounts, or want to pause the progress, you can also store the crawling progress to disk. 

<img width="764" alt="helper" src="https://github.com/the-marenga/sf-scrapbook-helper/assets/107524538/39dfbb4c-9166-46f0-85f7-d4e13aed7c97">

I only wrote this tool, because I needed to quickly get characters a full scrapbook to test the api parsing of that. As such, I put no effort into the UI, or the code quality. 
If you have any issues, let me know. Currently I do not expect anyone to actually use this.

## Building
- Install [Rust](https://rustup.rs/)
- Build this crate 
  ```
  git clone https://github.com/the-marenga/sf-scrapbook-helper.git
  cd sf-scrapbook-helper
  cargo run --release
  ```

### Windows
Should just build fine.

> If you just want to have the exe, you can download the newest pre-build version here: [release](https://github.com/the-marenga/sf-scrapbook-helper/releases).

### Linux 
I have not tested this on linux, but you may need to run this:
```
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

### Mac
Should just build fine. You may just need the xcode cli tools

## Troubleshooting
If you are using the tool on windows 11 and it hangs at startup, you can try to launch it from the desktop. For unknown reasons, that fixes the issue. [Details](https://github.com/the-marenga/sf-scrapbook-helper/issues/3)
