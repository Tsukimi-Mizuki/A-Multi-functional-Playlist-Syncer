using Avalonia.Controls;
using Avalonia.Interactivity;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.Views;

public partial class MessageDialog : Window
{
    public MessageDialog()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this);
    }

    public MessageDialog(string title, string message, string okText = "确定", string? cancelText = null)
        : this()
    {
        Title = title;
        TitleText.Text = title;
        MessageText.Text = message;
        OkButton.Content = okText;
        if (string.IsNullOrWhiteSpace(cancelText))
        {
            CancelButton.IsVisible = false;
        }
        else
        {
            CancelButton.IsVisible = true;
            CancelButton.Content = cancelText;
        }
    }

    private void OnOkClick(object? sender, RoutedEventArgs e) => Close(true);

    private void OnCancelClick(object? sender, RoutedEventArgs e) => Close(false);
}