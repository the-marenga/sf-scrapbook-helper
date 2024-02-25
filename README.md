# S&F Scrapbook Helper
A simple tool to help you fill the scrapbook by showing you the players with the most items, that you have not yet collected. 
You can either attack the best players manually (which might cost a mushroom), or click the automate button to battle the best character as soon, as it is free. 

Both normal and SSO (S&F Account) characters are supported.

The HoF will initially be fetched from a recent snapshot of the server. If you want a more recent version, you can crawl the server data yourself via the buttons on the left side. If you have multiple accounts, or want to pause the progress, you can store this crawling progress to disk and restore it at a later date. 

<img width="912" src="https://github.com/the-marenga/sf-scrapbook-helper/assets/107524538/bcd972fb-ebbc-4e1c-80de-1352b4b841aa">

## Privacy Notice
If you want to have your account data (username+equipment) removed from the online HoF data set, or you represent playagames and you have an issue with the HoF data being shared at all, feel free to open an issue, or contact me via:

`remove_hof@marenga.dev`


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
