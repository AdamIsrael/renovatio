# bootc plugin

Running the bootc plugin will prompt the user for their password twice by default. This is due to the policy kit configuration.

Create a new file: `/etc/polkit-1/rules.d/50-pkexec.rules` with the following contents:

```
polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.policykit.exec") {
        return polkit.Result.AUTH_ADMIN_KEEP;
    }
});
```

This will allow `pkexec` to cache the password for a duration, so the second prompt will not be required.
